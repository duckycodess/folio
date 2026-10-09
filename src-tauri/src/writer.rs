//! The native writer (issue #5). It applies a plan the `PlanRegistry` issued and the
//! user approved, records one durable outcome and one recoverable history entry per
//! operation, and reverses a whole batch through Undo after the user confirmed its
//! preview. It never rolls back on its own: a failure stops the batch, earlier changes
//! are kept, and Undo offers to reverse them (docs/contracts.md).

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use crate::contracts::{ActionPlan, Approval, BatchResult, FileOperation, HistoryEntry, UndoPreflight};
use crate::db::NativeResult;
use crate::error::{error, ErrorCode, FolioError};
use crate::extract::MAX_TEXT_BYTES;
use crate::identity::{content_hash, document_id as identity_of, is_editable_media_type};
use crate::index;
use crate::plan::{self, AttemptOutcome};
use crate::workspace::{self, ScopedRoot};

/// Previous content is kept for the edits of this many most recent plans.
pub const RECOVERABLE_PLANS: usize = 100;
/// `list_history` never returns more entries than this.
pub const MAX_HISTORY_ENTRIES: usize = 500;

fn unreadable(cause: std::io::Error, path: &Path) -> FolioError {
    error(ErrorCode::DocumentUnavailable, "A file could not be written.").with_detail("cause", cause.to_string()).with_detail("file", path.to_string_lossy())
}

fn hash_of(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| content_hash(&bytes))
}

/// Every physical change goes through this trait, so tests can make any step fail.
pub trait FileSystem {
    /// Replaces `path` with `bytes` only while it still hashes to `expected`.
    fn replace_checked(&self, path: &Path, bytes: &[u8], expected: &str) -> Result<(), FolioError>;
    /// Creates `path`; never replaces something already there.
    fn create_new(&self, path: &Path, bytes: &[u8]) -> Result<(), FolioError>;
    /// Moves `from` to `to`; never replaces something already at `to`.
    fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), FolioError>;
    /// Removes `path` only while it still hashes to `expected`.
    fn remove_checked(&self, path: &Path, expected: &str) -> Result<(), FolioError>;
}

pub struct RealFileSystem;

static TEMPORARY: AtomicU64 = AtomicU64::new(0);
/// Temporary files are named `.<file><TEMPORARY_MARK><pid>-<n>.tmp`.
const TEMPORARY_MARK: &str = ".folio-";
/// A temporary file this old belongs to a save that was interrupted (a crash or a kill).
const ABANDONED_AFTER: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Whether `name` is one of the writer's own temporary files.
fn is_temporary_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('.').and_then(|rest| rest.strip_suffix(".tmp")) else { return false };
    let Some((file, counters)) = rest.rsplit_once(TEMPORARY_MARK) else { return false };
    let numbered = counters.split_once('-').is_some_and(|(pid, n)| [pid, n].iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())));
    !file.is_empty() && numbered
}

/// Removes a temporary file an interrupted save left beside a document. Only the writer's
/// own naming pattern, only regular files, and only once it is old enough that no save
/// can still be using it. Returns whether `path` was removed.
pub fn remove_if_abandoned(path: &Path, metadata: &fs::Metadata) -> bool {
    let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
    let old_enough = metadata.modified().ok().and_then(|modified| modified.elapsed().ok()).is_some_and(|age| age >= ABANDONED_AFTER);
    metadata.is_file() && is_temporary_name(&name) && old_enough && fs::remove_file(path).is_ok()
}

impl FileSystem for RealFileSystem {
    /// Writes a hidden temporary file beside the target with the target's permissions,
    /// re-hashes the target just before swapping, then renames the temporary file over it.
    /// A file Folio may not write is refused on every platform, because on Unix renaming
    /// over it only needs write access to the folder. Ownership, ACLs and extended
    /// attributes are not carried over. The window between the re-hash and the rename is
    /// the only unguarded moment; it is not claimed atomic.
    fn replace_checked(&self, path: &Path, bytes: &[u8], expected: &str) -> Result<(), FolioError> {
        let permissions = fs::metadata(path).map_err(|cause| unreadable(cause, path))?.permissions();
        // Opening for writing without truncating changes nothing; it asks the OS whether
        // this process may write the file itself, which `readonly()` alone does not.
        let refused = if permissions.readonly() { Some("readOnly".to_owned()) } else { OpenOptions::new().write(true).open(path).err().map(|cause| cause.to_string()) };
        if let Some(cause) = refused {
            return Err(error(ErrorCode::DocumentUnavailable, "Folio is not allowed to change this file, so it left it as it is.").with_detail("file", path.to_string_lossy()).with_detail("cause", cause));
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let temporary = path.with_file_name(format!(".{name}{TEMPORARY_MARK}{}-{}.tmp", std::process::id(), TEMPORARY.fetch_add(1, Ordering::SeqCst)));
        let written = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::set_permissions(&temporary, permissions)
        })();
        if let Err(cause) = written {
            let _ = fs::remove_file(&temporary);
            return Err(unreadable(cause, path));
        }
        if hash_of(path).as_deref() != Some(expected) {
            let _ = fs::remove_file(&temporary);
            return Err(error(ErrorCode::TargetChanged, "This file changed while Folio was saving. It was left as it is.").with_detail("file", path.to_string_lossy()));
        }
        fs::rename(&temporary, path).map_err(|cause| {
            let _ = fs::remove_file(&temporary);
            unreadable(cause, path)
        })
    }

    fn create_new(&self, path: &Path, bytes: &[u8]) -> Result<(), FolioError> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path).map_err(|cause| match cause.kind() {
            ErrorKind::AlreadyExists => error(ErrorCode::DestinationExists, "Something already uses that name. The existing file was left alone."),
            _ => unreadable(cause, path),
        })?;
        if let Err(cause) = file.write_all(bytes).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(unreadable(cause, path));
        }
        Ok(())
    }

    /// A hard link fails if the destination exists, so nothing is ever overwritten; the
    /// source name is then removed. Filesystems without hard links (e.g. FAT) fall back
    /// to a checked rename, whose check-then-rename window is documented, not hidden.
    fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), FolioError> {
        let occupied = || error(ErrorCode::DestinationExists, "Something already uses that name. The existing file was left alone.").with_detail("file", to.to_string_lossy());
        match fs::hard_link(from, to) {
            Ok(()) => fs::remove_file(from).map_err(|cause| {
                let _ = fs::remove_file(to);
                unreadable(cause, from)
            }),
            Err(cause) if cause.kind() == ErrorKind::AlreadyExists => Err(occupied()),
            Err(_) => {
                if fs::symlink_metadata(to).is_ok() { return Err(occupied()); }
                fs::rename(from, to).map_err(|cause| unreadable(cause, from))
            }
        }
    }

    fn remove_checked(&self, path: &Path, expected: &str) -> Result<(), FolioError> {
        if hash_of(path).as_deref() != Some(expected) {
            return Err(error(ErrorCode::UndoConflict, "This file changed after Folio saved it, so it was left alone.").with_detail("file", path.to_string_lossy()));
        }
        fs::remove_file(path).map_err(|cause| unreadable(cause, path))
    }
}

/// What the writer reports for an apply: the frozen per-operation record, whether the
/// plan's own record and history pruning were stored after the files changed, and whether
/// the local index caught up with the files that changed.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub batch: BatchResult,
    pub history_settled: bool,
    pub index_refreshed: bool,
}

/// An Undo that stops partway leaves the rest pending; a fresh preview can finish it.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UndoReport {
    pub plan_id: String,
    pub undone_entry_ids: Vec<String>,
    pub remaining_entry_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FolioError>,
    pub index_refreshed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Edit,
    Create,
    Rename,
    Move,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Edit => "edit",
            Kind::Create => "create",
            Kind::Rename => "rename",
            Kind::Move => "move",
        }
    }

    fn parse(value: &str) -> Result<Kind, FolioError> {
        match value {
            "edit" => Ok(Kind::Edit),
            "create" => Ok(Kind::Create),
            "rename" => Ok(Kind::Rename),
            "move" => Ok(Kind::Move),
            other => Err(error(ErrorCode::HistoryUnknown, "A history entry has an unknown operation kind.").with_detail("kind", other)),
        }
    }
}

/// One applied operation, as stored.
struct Record {
    kind: Kind,
    document_ref: Option<String>,
    before_path: Option<String>,
    after_path: String,
    before_hash: Option<String>,
    after_hash: String,
    before_content: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanSummary {
    operations: Vec<OperationSummary>,
    impacts: Vec<crate::contracts::ImpactCandidate>,
}

/// An operation without its file body, so stored plans stay small.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationSummary {
    kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination_relative_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected_content_hash: Option<String>,
}

fn summarize(plan: &ActionPlan) -> PlanSummary {
    PlanSummary {
        operations: plan
            .operations
            .iter()
            .map(|operation| OperationSummary {
                kind: operation.kind().into(),
                relative_path: operation.source_path().map(str::to_owned),
                destination_relative_path: operation.destination_path().map(str::to_owned),
                expected_content_hash: operation.expected_content_hash().map(str::to_owned),
            })
            .collect(),
        impacts: plan.impacts.clone(),
    }
}

fn perform(root: &ScopedRoot, operation: &FileOperation, files: &dyn FileSystem) -> Result<Record, FolioError> {
    match operation {
        FileOperation::Edit { document_id, relative_path, expected_content_hash, after } => {
            let path = workspace::resolve_document(&root.path, relative_path)?;
            let before = workspace::read_bounded(&path, MAX_TEXT_BYTES)?;
            if content_hash(&before) != *expected_content_hash {
                return Err(error(ErrorCode::TargetChanged, "This file changed since the preview was prepared. It was left as it is.").with_detail("path", relative_path.as_str()));
            }
            files.replace_checked(&path, after.as_bytes(), expected_content_hash)?;
            Ok(Record {
                kind: Kind::Edit,
                document_ref: Some(document_id.clone()),
                before_path: Some(relative_path.clone()),
                after_path: relative_path.clone(),
                before_hash: Some(expected_content_hash.clone()),
                after_hash: content_hash(after.as_bytes()),
                before_content: Some(before),
            })
        }
        FileOperation::Create { destination_relative_path, content, .. } => {
            let path = plan::resolve_destination(&root.path, destination_relative_path)?;
            files.create_new(&path, content.as_bytes())?;
            Ok(Record {
                kind: Kind::Create,
                document_ref: Some(identity_of(&root.id, destination_relative_path)),
                before_path: None,
                after_path: destination_relative_path.clone(),
                before_hash: None,
                after_hash: content_hash(content.as_bytes()),
                before_content: None,
            })
        }
        FileOperation::Rename { document_id, relative_path, expected_content_hash, destination_relative_path, .. }
        | FileOperation::Move { document_id, relative_path, expected_content_hash, destination_relative_path, .. } => {
            let source = workspace::resolve_document(&root.path, relative_path)?;
            if hash_of(&source).as_deref() != Some(expected_content_hash.as_str()) {
                return Err(error(ErrorCode::TargetChanged, "This file changed since the preview was prepared. It was left as it is.").with_detail("path", relative_path.as_str()));
            }
            let destination = plan::resolve_destination(&root.path, destination_relative_path)?;
            files.rename_no_replace(&source, &destination)?;
            Ok(Record {
                kind: if matches!(operation, FileOperation::Rename { .. }) { Kind::Rename } else { Kind::Move },
                document_ref: Some(document_id.clone()),
                before_path: Some(relative_path.clone()),
                after_path: destination_relative_path.clone(),
                before_hash: Some(expected_content_hash.clone()),
                after_hash: expected_content_hash.clone(),
                before_content: None,
            })
        }
    }
}

fn record_history(conn: &Connection, plan_id: &str, index: usize, record: &Record, now: i64) -> NativeResult<String> {
    let id = format!("history-{plan_id}-{index}");
    conn.execute(
        "INSERT INTO history (id, plan_id, operation_index, operation_kind, document_ref, before_path, after_path, before_hash, after_hash, before_content, applied_at, recoverable) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1)",
        params![id, plan_id, index as i64, record.kind.as_str(), record.document_ref, record.before_path, record.after_path, record.before_hash, record.after_hash, record.before_content, now.to_string()],
    )?;
    Ok(id)
}

/// Applies an approved plan. The caller has just run `PlanRegistry::assert_can_apply`,
/// which checks the approval, the digest, expiry and every target and destination.
/// Each operation re-checks its own target as it runs; the first failure stops the
/// batch, a cancellation stops it before the next operation, and every completed
/// change keeps its history entry. The index is refreshed for the files that changed.
///
/// An `Err` means no file was changed. Once the first operation has run, the report is
/// always returned, and bookkeeping that fails afterwards is reported as
/// `history_settled: false` rather than as a failure of changes that did happen.
pub fn apply_plan(conn: &mut Connection, root: &ScopedRoot, plan: &ActionPlan, approval: &Approval, now: i64, files: &dyn FileSystem, cancel: &AtomicBool) -> NativeResult<ApplyReport> {
    // The duplicate check and both records commit together before the first write, so a
    // failed setup never leaves a plan that looks applied when nothing changed.
    let setup = conn.transaction()?;
    let recorded: Option<i64> = setup.query_row("SELECT 1 FROM action_plans WHERE id = ?1", [&plan.id], |row| row.get(0)).optional()?;
    if recorded.is_some() {
        return Err(error(ErrorCode::PlanStateInvalid, "This plan has already been applied. Review a fresh preview.").with_detail("planId", plan.id.as_str()));
    }
    setup.execute(
        "INSERT INTO action_plans (id, workspace_id, plan_json, plan_digest, status, created_at, expires_at, applied_at) VALUES (?1, ?2, ?3, ?4, 'approved', ?5, ?6, ?7)",
        params![plan.id, plan.workspace_id, serde_json::to_string(&summarize(plan))?, plan.digest, plan.created_at.to_string(), plan.expires_at.to_string(), now.to_string()],
    )?;
    setup.execute("INSERT INTO approvals (plan_id, plan_digest, approved_at) VALUES (?1, ?2, ?3)", params![plan.id, approval.plan_digest, approval.approved_at.to_string()])?;
    setup.commit()?;

    let mut attempts = Vec::new();
    let mut cancelled_after = None;
    let mut changed_paths: Vec<String> = Vec::new();
    for (index, operation) in plan.operations.iter().enumerate() {
        if index > 0 && cancel.load(Ordering::SeqCst) {
            cancelled_after = Some(index - 1);
            break;
        }
        match perform(root, operation, files) {
            Ok(record) => {
                changed_paths.extend(record.before_path.iter().cloned());
                changed_paths.push(record.after_path.clone());
                match record_history(conn, &plan.id, index, &record, now) {
                    Ok(history_entry_id) => attempts.push(AttemptOutcome::Succeeded { history_entry_id, completed_at: now }),
                    Err(failure) => {
                        // The file changed but its history could not be stored. Say so plainly.
                        attempts.push(AttemptOutcome::Failed {
                            error: error(ErrorCode::HistoryRequired, "The file was changed, but Folio could not record how to undo it.").with_detail("path", record.after_path.as_str()).with_detail("cause", failure.message),
                            completed_at: now,
                        });
                        break;
                    }
                }
            }
            Err(failure) => {
                attempts.push(AttemptOutcome::Failed { error: failure, completed_at: now });
                break;
            }
        }
    }

    // The attempts were built above in exactly the shape `settle_batch` accepts (one per
    // operation run, stopping at the first failure, a cancellation only after a finished
    // operation), and `assert_can_apply` matched the approval before this ran.
    let batch = plan::settle_batch(plan, approval, &attempts, cancelled_after, now, now).expect("the writer reports attempts in the shape settle_batch requires");
    let settled = (|| -> NativeResult<()> {
        let status = match batch.stop_reason {
            crate::contracts::BatchStopReason::Failed => "failed",
            _ => "applied",
        };
        let stop_reason = serde_json::to_value(batch.stop_reason)?.as_str().unwrap_or_default().to_owned();
        conn.execute("UPDATE action_plans SET status = ?1, stop_reason = ?2 WHERE id = ?3", params![status, stop_reason, plan.id])?;
        prune_history(conn, &root.id, RECOVERABLE_PLANS)
    })();
    if let Err(failure) = &settled {
        eprintln!("Folio applied plan {} but could not finish recording it: {}", plan.id, failure.message);
    }
    changed_paths.dedup();
    let index_refreshed = changed_paths.is_empty() || index::refresh_paths(conn, root, &changed_paths).is_ok();
    Ok(ApplyReport { batch, history_settled: settled.is_ok(), index_refreshed })
}

/// Keeps the previous content of edits for the most recent `keep` plans. Older edits stay
/// listed but are no longer recoverable; renames, moves and creates need no stored content
/// and stay undoable.
pub fn prune_history(conn: &Connection, workspace_id: &str, keep: usize) -> NativeResult<()> {
    conn.execute(
        "UPDATE history SET before_content = NULL, recoverable = 0 WHERE operation_kind = 'edit' AND recoverable = 1 AND plan_id IN (SELECT id FROM action_plans WHERE workspace_id = ?1 AND applied_at IS NOT NULL ORDER BY CAST(applied_at AS INTEGER) DESC, rowid DESC LIMIT -1 OFFSET ?2)",
        params![workspace_id, keep as i64],
    )?;
    Ok(())
}

const HISTORY_COLUMNS: &str = "h.id, h.plan_id, h.operation_index, h.applied_at, h.document_ref, h.before_path, h.after_path, h.before_hash, h.after_hash, h.recoverable, h.undone_at";

fn entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let applied: String = row.get(3)?;
    let undone: Option<String> = row.get(10)?;
    Ok(HistoryEntry {
        id: row.get(0)?,
        plan_id: row.get(1)?,
        operation_index: row.get::<_, i64>(2)? as usize,
        applied_at: applied.parse().unwrap_or_default(),
        document_id: row.get(4)?,
        before_relative_path: row.get(5)?,
        after_relative_path: row.get(6)?,
        before_content_hash: row.get(7)?,
        after_content_hash: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        recoverable: row.get::<_, i64>(9)? != 0,
        undone_at: undone.and_then(|value| value.parse().ok()),
    })
}

/// The most recent history entries of a workspace, newest plan first, bounded by `limit`.
pub fn list_history(conn: &Connection, workspace_id: &str, limit: usize) -> NativeResult<Vec<HistoryEntry>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {HISTORY_COLUMNS} FROM history h JOIN action_plans p ON p.id = h.plan_id WHERE p.workspace_id = ?1 ORDER BY CAST(h.applied_at AS INTEGER) DESC, p.rowid DESC, h.operation_index LIMIT ?2"
    ))?;
    let rows = statement.query_map(params![workspace_id, limit.clamp(1, MAX_HISTORY_ENTRIES) as i64], entry_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

struct StoredEntry {
    entry: HistoryEntry,
    kind: Kind,
    before_content: Option<Vec<u8>>,
}

fn plan_entries(conn: &Connection, workspace_id: &str, plan_id: &str) -> NativeResult<Vec<StoredEntry>> {
    let mut statement = conn.prepare(&format!(
        "SELECT {HISTORY_COLUMNS}, h.operation_kind, h.before_content FROM history h JOIN action_plans p ON p.id = h.plan_id WHERE p.workspace_id = ?1 AND h.plan_id = ?2 ORDER BY h.operation_index"
    ))?;
    let rows = statement.query_map(params![workspace_id, plan_id], |row| Ok((entry_from_row(row)?, row.get::<_, String>(11)?, row.get::<_, Option<Vec<u8>>>(12)?)))?;
    let mut entries = Vec::new();
    for row in rows {
        let (entry, kind, before_content) = row?;
        entries.push(StoredEntry { entry, kind: Kind::parse(&kind)?, before_content });
    }
    if entries.is_empty() {
        return Err(error(ErrorCode::HistoryUnknown, "Folio has no recorded changes for that plan.").with_detail("planId", plan_id));
    }
    Ok(entries)
}

/// The Undo preview: which entries would be reversed and anything that blocks them.
/// Nothing is written.
pub fn preview_undo(conn: &Connection, root: &ScopedRoot, plan_id: &str) -> NativeResult<UndoPreflight> {
    let entries: Vec<HistoryEntry> = plan_entries(conn, &root.id, plan_id)?.into_iter().map(|stored| stored.entry).collect();
    Ok(plan::preflight_undo(&root.path, plan_id, &entries))
}

/// Reverses every pending entry of an applied plan, newest first, after the user confirmed
/// exactly the entries the preview listed. If anything conflicts nothing is written. If a
/// step fails partway, the entries already reversed stay reversed and the rest remain
/// pending, so a fresh preview can finish the Undo.
pub fn undo_plan(conn: &mut Connection, root: &ScopedRoot, plan_id: &str, confirmed_entry_ids: &[String], now: i64, files: &dyn FileSystem) -> NativeResult<UndoReport> {
    let stored = plan_entries(conn, &root.id, plan_id)?;
    let entries: Vec<HistoryEntry> = stored.iter().map(|stored| stored.entry.clone()).collect();
    let preflight = plan::preflight_undo(&root.path, plan_id, &entries);
    if preflight.entry_ids.is_empty() {
        return Err(error(ErrorCode::PlanStateInvalid, "Everything in this change has already been undone.").with_detail("planId", plan_id));
    }
    let mut shown = confirmed_entry_ids.to_vec();
    let mut current = preflight.entry_ids.clone();
    shown.sort();
    current.sort();
    if shown != current {
        return Err(error(ErrorCode::ApprovalStale, "What Undo would change is different from the preview you confirmed. Review the Undo preview again.").with_detail("planId", plan_id));
    }
    plan::assert_undoable(&preflight)?;

    let mut undone = Vec::new();
    let mut failure = None;
    let mut changed_paths: Vec<String> = Vec::new();
    for item in stored.iter().rev().filter(|item| item.entry.undone_at.is_none()) {
        let entry = &item.entry;
        let applied = entry.after_relative_path.as_deref().unwrap_or_default();
        let result = (|| -> Result<(), FolioError> {
            let current = workspace::resolve_document(&root.path, applied)?;
            match item.kind {
                Kind::Edit => {
                    let previous = item.before_content.as_deref().ok_or_else(|| error(ErrorCode::UndoConflict, "Folio no longer keeps the previous version of this file.").with_detail("blockingRelativePath", applied))?;
                    files.replace_checked(&current, previous, &entry.after_content_hash)
                }
                Kind::Create => files.remove_checked(&current, &entry.after_content_hash),
                Kind::Rename | Kind::Move => {
                    let original = entry.before_relative_path.as_deref().unwrap_or_default();
                    files.rename_no_replace(&current, &plan::resolve_destination(&root.path, original)?)
                }
            }
        })();
        match result {
            Ok(()) => {
                conn.execute("UPDATE history SET undone_at = ?1 WHERE id = ?2", params![now.to_string(), entry.id])?;
                changed_paths.extend(entry.before_relative_path.iter().cloned());
                changed_paths.push(applied.to_owned());
                undone.push(entry.id.clone());
            }
            Err(cause) => {
                failure = Some(cause);
                break;
            }
        }
    }
    changed_paths.dedup();
    let index_refreshed = changed_paths.is_empty() || index::refresh_paths(conn, root, &changed_paths).is_ok();
    let remaining = preflight.entry_ids.iter().filter(|id| !undone.contains(id)).cloned().collect();
    Ok(UndoReport { plan_id: plan_id.into(), undone_entry_ids: undone, remaining_entry_ids: remaining, error: failure, index_refreshed })
}

/// Builds an edit that replaces one passage, for interpreters that name the text to change
/// rather than the whole new file. The passage must occur exactly once.
pub fn passage_edit(conn: &Connection, root: &ScopedRoot, document_id: &str, before: &str, after: &str) -> NativeResult<FileOperation> {
    let document = index::get_document(conn, &root.id, document_id)?;
    if !is_editable_media_type(&document.media_type) {
        return Err(error(ErrorCode::UnsupportedMediaType, "Folio edits TXT and Markdown files. Text-based PDFs are read-only.").with_detail("path", document.relative_path.as_str()));
    }
    let current = workspace::read_text(&root.path, &document.relative_path)?;
    let unsupported = |reason: &str| error(ErrorCode::OperationUnsupported, "Choose the exact text to change; it must appear once in the file.").with_detail("path", document.relative_path.as_str()).with_detail("reason", reason);
    if before.is_empty() || before == after {
        return Err(unsupported("emptyOrUnchanged"));
    }
    match current.content.matches(before).count() {
        0 => return Err(unsupported("passageNotFound")),
        1 => {}
        count => return Err(unsupported("passageAmbiguous").with_detail("occurrences", count.to_string())),
    }
    Ok(FileOperation::Edit {
        document_id: document.id,
        relative_path: document.relative_path,
        expected_content_hash: current.content_hash,
        after: current.content.replacen(before, after, 1),
    })
}

/// The applied plan a history entry belongs to, if it is in this workspace.
#[allow(dead_code)]
pub fn plan_workspace(conn: &Connection, plan_id: &str) -> NativeResult<Option<String>> {
    Ok(conn.query_row("SELECT workspace_id FROM action_plans WHERE id = ?1", [plan_id], |row| row.get(0)).optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{BatchStopReason, DestinationState, ImpactStrength, OperationStatus};
    use crate::index::tests::{assert_located, fixture_workspace, id_of, scan};
    use crate::plan::PlanRegistry;
    use crate::ripple;
    use std::cell::Cell;
    use std::collections::BTreeMap;

    const NOW: i64 = 1_760_000_000_000;
    const LIFETIME: i64 = 5 * 60 * 1000;
    static CREATED: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

    fn hash_all(folder: &Path) -> BTreeMap<String, String> {
        walkdir::WalkDir::new(folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| (entry.path().strip_prefix(folder).unwrap().to_string_lossy().replace('\\', "/"), content_hash(&fs::read(entry.path()).unwrap())))
            .collect()
    }

    fn current(conn: &Connection, root: &ScopedRoot, path: &str) -> String {
        index::get_document(conn, &root.id, &id_of(root, path)).unwrap().content_hash
    }

    fn edit(conn: &Connection, root: &ScopedRoot, path: &str, before: &str, after: &str) -> FileOperation {
        passage_edit(conn, root, &id_of(root, path), before, after).unwrap()
    }

    fn relocate(conn: &Connection, root: &ScopedRoot, path: &str, destination: &str, rename: bool) -> FileOperation {
        let (document_id, relative_path, expected_content_hash, destination_relative_path) = (id_of(root, path), path.to_owned(), current(conn, root, path), destination.to_owned());
        let expected_destination = DestinationState::Absent;
        if rename {
            FileOperation::Rename { document_id, relative_path, expected_content_hash, destination_relative_path, expected_destination }
        } else {
            FileOperation::Move { document_id, relative_path, expected_content_hash, destination_relative_path, expected_destination }
        }
    }

    fn create(path: &str, content: &str) -> FileOperation {
        FileOperation::Create { destination_relative_path: path.into(), media_type: "text/markdown".into(), content: content.into(), expected_destination: DestinationState::Absent }
    }

    /// The command sequence the UI drives: prepare, preflight, approve, gate, apply.
    fn approved(conn: &Connection, root: &ScopedRoot, registry: &mut PlanRegistry, operations: Vec<FileOperation>) -> (ActionPlan, Approval) {
        let impacts = ripple::plan_impacts(conn, root, &operations).unwrap();
        // Each test plan gets its own creation time, as plans prepared in separate sessions would.
        let created = NOW - 1000 + CREATED.fetch_add(1, Ordering::SeqCst) % 1000;
        let plan = registry.prepare(&root.id, operations, impacts, created, LIFETIME).unwrap();
        plan::preflight_plan(&root.path, &plan, NOW).unwrap();
        let approval = registry.approve(&plan.id, &plan.digest, NOW + 1).unwrap();
        (plan, approval)
    }

    fn apply_with(conn: &mut Connection, root: &ScopedRoot, operations: Vec<FileOperation>, files: &dyn FileSystem) -> ApplyReport {
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(conn, root, &mut registry, operations);
        registry.assert_can_apply(&root.path, &plan.id, NOW + 2).unwrap();
        apply_plan(conn, root, &plan, &approval, NOW + 2, files, &AtomicBool::new(false)).unwrap()
    }

    fn statuses(report: &ApplyReport) -> Vec<OperationStatus> {
        report.batch.outcomes.iter().map(|outcome| outcome.status).collect()
    }

    fn indexed_text(conn: &Connection, root: &ScopedRoot, path: &str) -> String {
        conn.query_row("SELECT group_concat(chunk_text, '\n') FROM chunks WHERE document_id = ?1", [id_of(root, path)], |row| row.get(0)).unwrap()
    }

    fn undo_all(conn: &mut Connection, root: &ScopedRoot, plan_id: &str, files: &dyn FileSystem) -> NativeResult<UndoReport> {
        let preview = preview_undo(conn, root, plan_id)?;
        undo_plan(conn, root, plan_id, &preview.entry_ids, NOW + 10, files)
    }

    /// Fails the Nth physical step (1-based) and delegates the rest.
    struct Failing {
        fail_on: usize,
        calls: Cell<usize>,
        cancel_after_first: Option<&'static AtomicBool>,
    }

    impl Failing {
        fn on(step: usize) -> Self {
            Failing { fail_on: step, calls: Cell::new(0), cancel_after_first: None }
        }

        fn tick(&self) -> Result<(), FolioError> {
            self.calls.set(self.calls.get() + 1);
            if let Some(flag) = self.cancel_after_first { flag.store(true, Ordering::SeqCst); }
            if self.calls.get() == self.fail_on { Err(error(ErrorCode::DocumentUnavailable, "injected write failure")) } else { Ok(()) }
        }
    }

    impl FileSystem for Failing {
        fn replace_checked(&self, path: &Path, bytes: &[u8], expected: &str) -> Result<(), FolioError> { self.tick()?; RealFileSystem.replace_checked(path, bytes, expected) }
        fn create_new(&self, path: &Path, bytes: &[u8]) -> Result<(), FolioError> { self.tick()?; RealFileSystem.create_new(path, bytes) }
        fn rename_no_replace(&self, from: &Path, to: &Path) -> Result<(), FolioError> { self.tick()?; RealFileSystem.rename_no_replace(from, to) }
        fn remove_checked(&self, path: &Path, expected: &str) -> Result<(), FolioError> { self.tick()?; RealFileSystem.remove_checked(path, expected) }
    }

    #[test]
    fn ripple_deadline_case_flags_related_passages_and_changes_only_the_target() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let operation = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![operation]);
        assert_eq!(hash_all(folder.path()), before, "preparing and approving write nothing");

        let evidence: Vec<&str> = plan.impacts.iter().filter(|impact| impact.strength == ImpactStrength::Evidence).map(|impact| impact.relative_path.as_str()).collect();
        assert_eq!(evidence, vec!["meetings/meeting-notes.md", "notes/tala-sa-proyekto.md", "projects/submission-checklist.md"]);
        for impact in plan.impacts.iter().filter(|impact| impact.strength == ImpactStrength::Evidence) {
            assert_eq!(impact.relationship_type, Some(crate::contracts::RelationshipKind::ExplicitReference));
            for passage in &impact.evidence {
                assert!(passage.text.contains("October 20"));
                assert_located(folder.path(), &impact.relative_path, passage);
            }
        }
        let similar: Vec<&str> = plan.impacts.iter().filter(|impact| impact.strength == ImpactStrength::SimilarityOnly).map(|impact| impact.relative_path.as_str()).collect();
        assert_eq!(similar, vec!["archive/project-plan-copy.md"]);
        assert!(!plan.impacts.iter().any(|impact| impact.relative_path.starts_with("courses/")), "same date, unrelated event");

        registry.assert_can_apply(&root.path, &plan.id, NOW + 2).unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!((report.batch.stop_reason, report.index_refreshed), (BatchStopReason::Completed, true));
        let after = hash_all(folder.path());
        let changed: Vec<&String> = after.keys().filter(|path| before.get(*path) != after.get(*path)).collect();
        assert_eq!(changed, vec!["projects/project-plan.md"]);
        let indexed = indexed_text(&conn, &root, "projects/project-plan.md");
        assert!(indexed.contains("October 23") && !indexed.contains("October 20"));
        assert_eq!(list_history(&conn, &root.id, 10).unwrap().len(), 1);
    }

    #[test]
    fn ripple_reads_filipino_dates_and_shared_facts_without_matching_other_years() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::write(folder.path().join("notes/oktubre.md"), "# Paalala\n\nAng pasahan ay Oktubre 20. Tingnan ang [plano](../projects/project-plan.md).\n").unwrap();
        fs::write(folder.path().join("notes/kumperensya.md"), "# Kumperensya\n\nSa October 2026 ang kumperensya. Tingnan ang [plano](../projects/project-plan.md).\n").unwrap();
        scan(&mut conn, &root);
        let target = id_of(&root, "projects/project-plan.md");
        let fact = id_of(&root, "research/review-reminders.md");
        fs::write(folder.path().join("research/review-reminders.md"), "# Review reminders\n\nSubmit by October 20.\n").unwrap();
        scan(&mut conn, &root);
        conn.execute(
            "INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES ('fact', ?1, ?2, 'sharedFactCandidate', '[]', 'model', 0.7, 'x', 'y', '0')",
            [&fact, &target],
        )
        .unwrap();
        let impacts = ripple::plan_impacts(&conn, &root, &[edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]).unwrap();
        let find = |path: &str| impacts.iter().find(|impact| impact.relative_path == path);
        assert_eq!(find("notes/oktubre.md").unwrap().strength, ImpactStrength::Evidence);
        assert!(find("notes/kumperensya.md").is_none(), "October 2026 is not October 20");
        let shared = find("research/review-reminders.md").unwrap();
        assert_eq!((shared.strength, shared.relationship_type), (ImpactStrength::Evidence, Some(crate::contracts::RelationshipKind::SharedFactCandidate)));
    }

    #[test]
    fn create_rename_and_move_change_real_files_and_record_history() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let operations = vec![
            create("notes/bagong-tala.md", "# Bagong tala\n\nPaalala tungkol sa xylophone practice.\n"),
            relocate(&conn, &root, "projects/submission-checklist.md", "projects/checklist.md", true),
            relocate(&conn, &root, "personal/grocery-list.md", "archive/grocery-list.md", false),
        ];
        let report = apply_with(&mut conn, &root, operations, &RealFileSystem);
        assert_eq!(statuses(&report), vec![OperationStatus::Succeeded; 3]);
        assert!(folder.path().join("notes/bagong-tala.md").is_file());
        assert!(folder.path().join("projects/checklist.md").is_file() && !folder.path().join("projects/submission-checklist.md").exists());
        assert!(folder.path().join("archive/grocery-list.md").is_file() && !folder.path().join("personal/grocery-list.md").exists());
        assert_eq!(index::search(&conn, &root.id, "xylophone", 5).unwrap()[0].document.relative_path, "notes/bagong-tala.md");
        let history = list_history(&conn, &root.id, 10).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[1].before_relative_path.as_deref(), Some("projects/submission-checklist.md"));
        assert_eq!(history[1].document_id.as_deref(), Some(id_of(&root, "projects/submission-checklist.md").as_str()), "history names the identity that changed");
        let rescan = scan(&mut conn, &root);
        assert_eq!(rescan.added + rescan.updated + rescan.removed, 0, "the index already matched the disk");
    }

    #[test]
    fn a_failed_operation_stops_the_batch_and_keeps_earlier_changes_for_undo() {
        for (failing_kind, second) in ["edit", "rename", "move", "create"].into_iter().enumerate() {
            let (folder, mut conn, root) = fixture_workspace();
            scan(&mut conn, &root);
            let first = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
            let operation = match second {
                "edit" => edit(&conn, &root, "meetings/meeting-notes.md", "October 20", "October 23"),
                "rename" => relocate(&conn, &root, "notes/paalala.md", "notes/mga-paalala.md", true),
                "move" => relocate(&conn, &root, "notes/paalala.md", "archive/paalala.md", false),
                _ => create("notes/bago.md", "# Bago\n"),
            };
            let untouched = hash_all(folder.path());
            let report = { let operations = vec![first, operation, create("notes/third.md", "x")]; apply_with(&mut conn, &root, operations, &Failing::on(2)) };
            assert_eq!(statuses(&report), vec![OperationStatus::Succeeded, OperationStatus::Failed, OperationStatus::NotStarted], "{second} ({failing_kind})");
            assert_eq!(report.batch.stop_reason, BatchStopReason::Failed);
            let after = hash_all(folder.path());
            let changed: Vec<&String> = after.keys().filter(|path| untouched.get(*path) != after.get(*path)).collect();
            assert_eq!(changed, vec!["projects/project-plan.md"], "only the completed change exists ({second})");
            assert!(indexed_text(&conn, &root, "projects/project-plan.md").contains("October 23"), "the completed change is indexed");
            undo_all(&mut conn, &root, &report.batch.plan_id, &RealFileSystem).unwrap();
            assert_eq!(hash_all(folder.path()), untouched, "Undo reverses the kept change ({second})");
        }
    }

    #[test]
    fn cancellation_finishes_the_running_operation_and_stops_before_the_next() {
        static CANCEL: AtomicBool = AtomicBool::new(false);
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let operations = vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"), create("notes/later.md", "x")];
        let (plan, approval) = approved(&conn, &root, &mut registry, operations);
        let files = Failing { fail_on: usize::MAX, calls: Cell::new(0), cancel_after_first: Some(&CANCEL) };
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &files, &CANCEL).unwrap();
        assert_eq!(statuses(&report), vec![OperationStatus::Succeeded, OperationStatus::Cancelled]);
        assert_eq!(report.batch.stop_reason, BatchStopReason::Cancelled);
        assert!(!folder.path().join("notes/later.md").exists());
    }

    #[test]
    fn a_destination_that_appears_after_preflight_is_never_overwritten() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![relocate(&conn, &root, "notes/paalala.md", "notes/mga-paalala.md", true), create("notes/new.md", "Folio")]);
        registry.assert_can_apply(&root.path, &plan.id, NOW + 2).unwrap();
        fs::write(folder.path().join("notes/mga-paalala.md"), "someone else's file").unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!(statuses(&report), vec![OperationStatus::Failed, OperationStatus::NotStarted]);
        assert_eq!(report.batch.outcomes[0].error.as_ref().unwrap().code, ErrorCode::DestinationExists);
        assert_eq!(fs::read_to_string(folder.path().join("notes/mga-paalala.md")).unwrap(), "someone else's file");
        assert!(folder.path().join("notes/paalala.md").is_file());

        fs::write(folder.path().join("notes/late.md"), "appeared late").unwrap();
        assert_eq!(RealFileSystem.create_new(&folder.path().join("notes/late.md"), b"Folio").unwrap_err().code, ErrorCode::DestinationExists);
        assert_eq!(fs::read_to_string(folder.path().join("notes/late.md")).unwrap(), "appeared late");
    }

    #[test]
    fn an_external_edit_after_preflight_is_kept() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]);
        registry.assert_can_apply(&root.path, &plan.id, NOW + 2).unwrap();
        fs::write(folder.path().join("projects/project-plan.md"), "edited elsewhere").unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.batch.outcomes[0].error.as_ref().unwrap().code, ErrorCode::TargetChanged);
        assert_eq!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap(), "edited elsewhere");
        assert!(list_history(&conn, &root.id, 10).unwrap().is_empty());
        let file = folder.path().join("notes/paalala.md");
        let expected = content_hash(&fs::read(&file).unwrap());
        fs::write(&file, "changed between the re-hash and the swap").unwrap();
        assert_eq!(RealFileSystem.replace_checked(&file, b"Folio", &expected).unwrap_err().code, ErrorCode::TargetChanged);
        assert_eq!(fs::read_to_string(&file).unwrap(), "changed between the re-hash and the swap");
        assert!(fs::read_dir(file.parent().unwrap()).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")), "no temporary file is left behind");
    }

    #[test]
    fn losing_access_to_the_folder_or_file_fails_honestly() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]);
        let target = folder.path().join("projects/project-plan.md");
        let original = fs::read(&target).unwrap();
        #[cfg(windows)]
        let locked = {
            let mut permissions = fs::metadata(&target).unwrap().permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&target, permissions).unwrap();
            target.clone()
        };
        #[cfg(unix)]
        let locked = {
            use std::os::unix::fs::PermissionsExt;
            let directory = target.parent().unwrap().to_path_buf();
            let mut folder_permissions = fs::metadata(&directory).unwrap().permissions();
            folder_permissions.set_mode(0o555);
            fs::set_permissions(&directory, folder_permissions).unwrap();
            directory
        };
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        let mut restore = fs::metadata(&locked).unwrap().permissions();
        #[cfg(windows)]
        restore.set_readonly(false);
        #[cfg(unix)]
        std::os::unix::fs::PermissionsExt::set_mode(&mut restore, 0o755);
        fs::set_permissions(&locked, restore).unwrap();
        assert_eq!(statuses(&report), vec![OperationStatus::Failed]);
        assert_eq!(fs::read(&target).unwrap(), original, "nothing was written");
        assert!(list_history(&conn, &root.id, 10).unwrap().is_empty());

        let (gone_folder, mut gone_conn, gone_root) = fixture_workspace();
        scan(&mut gone_conn, &gone_root);
        let mut gone_registry = PlanRegistry::new();
        let (gone_plan, _) = approved(&gone_conn, &gone_root, &mut gone_registry, vec![create("notes/x.md", "x")]);
        drop(gone_folder);
        assert!(gone_registry.assert_can_apply(&gone_root.path, &gone_plan.id, NOW + 2).is_err(), "a folder that disappeared is refused before any write");
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn edit_and_undo_keep_a_private_file_private() {
        use std::os::unix::fs::PermissionsExt;
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let target = folder.path().join("projects/project-plan.md");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let operation = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
        let report = apply_with(&mut conn, &root, vec![operation], &RealFileSystem);
        assert_eq!(statuses(&report), vec![OperationStatus::Succeeded]);
        assert_eq!(mode(&target), 0o600);
        undo_all(&mut conn, &root, &report.batch.plan_id, &RealFileSystem).unwrap();
        assert_eq!(mode(&target), 0o600, "Undo keeps them too");
    }

    #[cfg(unix)]
    #[test]
    fn edit_refuses_a_read_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let target = folder.path().join("projects/project-plan.md");
        let original = fs::read(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).unwrap();
        let operation = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
        let report = apply_with(&mut conn, &root, vec![operation], &RealFileSystem);
        assert_eq!(statuses(&report), vec![OperationStatus::Failed]);
        assert_eq!(report.batch.outcomes[0].error.as_ref().unwrap().code, ErrorCode::DocumentUnavailable);
        assert_eq!(fs::read(&target).unwrap(), original, "a read-only file is never replaced");
        assert_eq!(mode(&target), 0o444);
        assert!(fs::read_dir(target.parent().unwrap()).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().ends_with(".tmp")), "no temporary file is left behind");
    }

    #[test]
    fn a_rename_or_move_whose_source_changed_after_preflight_is_refused() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![relocate(&conn, &root, "notes/paalala.md", "archive/paalala.md", false)]);
        registry.assert_can_apply(&root.path, &plan.id, NOW + 2).unwrap();
        fs::write(folder.path().join("notes/paalala.md"), "edited elsewhere").unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.batch.outcomes[0].error.as_ref().unwrap().code, ErrorCode::TargetChanged);
        assert!(!folder.path().join("archive/paalala.md").exists());
        assert_eq!(fs::read_to_string(folder.path().join("notes/paalala.md")).unwrap(), "edited elsewhere");
        assert!(list_history(&conn, &root.id, 10).unwrap().is_empty());
    }

    #[test]
    fn a_failed_setup_changes_nothing_and_leaves_the_plan_free_to_apply() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]);
        let untouched = hash_all(folder.path());
        conn.execute_batch("CREATE TEMP TRIGGER refuse_approval BEFORE INSERT ON approvals BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        assert!(apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).is_err());
        assert_eq!(hash_all(folder.path()), untouched, "nothing was written");
        let recorded: i64 = conn.query_row("SELECT count(*) FROM action_plans WHERE id = ?1", [&plan.id], |row| row.get(0)).unwrap();
        assert_eq!(recorded, 0, "the plan record was rolled back with the approval");
        conn.execute_batch("DROP TRIGGER refuse_approval;").unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!(statuses(&report), vec![OperationStatus::Succeeded]);
    }

    #[test]
    fn bookkeeping_that_fails_after_the_writes_still_reports_what_changed() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let mut registry = PlanRegistry::new();
        let (plan, approval) = approved(&conn, &root, &mut registry, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]);
        conn.execute_batch("CREATE TEMP TRIGGER refuse_settling BEFORE UPDATE ON action_plans BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        let report = apply_plan(&mut conn, &root, &plan, &approval, NOW + 2, &RealFileSystem, &AtomicBool::new(false)).unwrap();
        assert_eq!(statuses(&report), vec![OperationStatus::Succeeded]);
        assert!(!report.history_settled && report.index_refreshed);
        assert!(report.batch.outcomes[0].history_entry_id.is_some(), "the caller still learns which entry Undo reverses");
        assert!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap().contains("October 23"));
        assert_eq!(list_history(&conn, &root.id, 10).unwrap().len(), 1);
    }

    #[test]
    fn a_scan_removes_only_temporary_files_an_interrupted_save_left_behind() {
        let (folder, mut conn, root) = fixture_workspace();
        let notes = folder.path().join("notes");
        let abandoned = notes.join(".paalala.md.folio-4242-0.tmp");
        let recent = notes.join(".paalala.md.folio-4242-1.tmp");
        let users = notes.join(".paalala.md.tmp");
        for path in [&abandoned, &recent, &users] {
            fs::write(path, "partial").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60 * 60);
        for path in [&abandoned, &users] {
            fs::File::options().write(true).open(path).unwrap().set_modified(old).unwrap();
        }
        scan(&mut conn, &root);
        assert!(!abandoned.exists());
        assert!(recent.exists(), "a save may still be using a recent one");
        assert!(users.exists(), "only the writer's own naming pattern is removed");
        assert!(is_temporary_name(".project-plan.md.folio-12-3.tmp"));
        assert!(!is_temporary_name(".folio-12-3.tmp") && !is_temporary_name(".notes.folio-x-3.tmp") && !is_temporary_name("notes.folio-12-3.tmp"));
    }

    #[test]
    fn ripple_does_not_read_the_filipino_word_may_as_the_month() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::write(folder.path().join("notes/klase.md"), "# Klase\n\nAng klase ay may 20 estudyante. Tingnan ang [plano](../projects/project-plan.md).\n").unwrap();
        fs::write(folder.path().join("notes/pasahan.md"), "# Pasahan\n\nAng pasahan ay sa May 20. Tingnan ang [plano](../projects/project-plan.md).\n").unwrap();
        scan(&mut conn, &root);
        let target = index::get_document(&conn, &root.id, &id_of(&root, "projects/project-plan.md")).unwrap();
        let flagged = |phrase: &str| ripple::impacts(&conn, &root.id, &target, phrase).unwrap().into_iter().map(|impact| impact.relative_path).collect::<Vec<_>>();
        let may = flagged("May 20");
        assert!(may.contains(&"notes/pasahan.md".to_owned()));
        assert!(!may.contains(&"notes/klase.md".to_owned()), "\"may 20 estudyante\" means there are 20 students");
        assert!(flagged("may 20").contains(&"notes/klase.md".to_owned()), "the Filipino phrase itself still matches when it is what changed");
    }

    #[test]
    fn undo_needs_a_confirmed_preview_and_refuses_after_external_edits() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let operations = vec![
            edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"),
            relocate(&conn, &root, "personal/grocery-list.md", "archive/grocery-list.md", false),
            create("notes/new.md", "# New\n"),
        ];
        let report = apply_with(&mut conn, &root, operations, &RealFileSystem);
        let plan_id = report.batch.plan_id.clone();
        let preview = preview_undo(&conn, &root, &plan_id).unwrap();
        assert!(preview.undoable);
        assert_eq!(undo_plan(&mut conn, &root, &plan_id, &preview.entry_ids[..1], NOW + 10, &RealFileSystem).unwrap_err().code, ErrorCode::ApprovalStale, "Undo only runs on the preview that was confirmed");

        fs::write(folder.path().join("notes/new.md"), "# New\n\nUser kept writing here.\n").unwrap();
        let blocked = preview_undo(&conn, &root, &plan_id).unwrap();
        assert!(!blocked.undoable);
        assert_eq!(undo_plan(&mut conn, &root, &plan_id, &blocked.entry_ids, NOW + 10, &RealFileSystem).unwrap_err().code, ErrorCode::UndoConflict);
        assert!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap().contains("October 23"), "nothing was undone");
        assert!(fs::read_to_string(folder.path().join("notes/new.md")).unwrap().contains("User kept writing"));

        fs::write(folder.path().join("notes/new.md"), "# New\n").unwrap();
        let undone = undo_all(&mut conn, &root, &plan_id, &RealFileSystem).unwrap();
        assert!(undone.error.is_none() && undone.remaining_entry_ids.is_empty() && undone.index_refreshed);
        assert_eq!(hash_all(folder.path()), before);
        assert!(indexed_text(&conn, &root, "projects/project-plan.md").contains("October 20"));
        assert_eq!(undo_all(&mut conn, &root, &plan_id, &RealFileSystem).unwrap_err().code, ErrorCode::PlanStateInvalid);
    }

    #[test]
    fn an_undo_that_stops_partway_can_be_finished() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let report = { let operations = vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"), create("notes/new.md", "# New\n")]; apply_with(&mut conn, &root, operations, &RealFileSystem) };
        let plan_id = report.batch.plan_id;
        let preview = preview_undo(&conn, &root, &plan_id).unwrap();
        let partial = undo_plan(&mut conn, &root, &plan_id, &preview.entry_ids, NOW + 10, &Failing::on(2)).unwrap();
        assert_eq!((partial.undone_entry_ids.len(), partial.remaining_entry_ids.len()), (1, 1));
        assert!(partial.error.is_some());
        let rest = undo_all(&mut conn, &root, &plan_id, &RealFileSystem).unwrap();
        assert_eq!(rest.undone_entry_ids, partial.remaining_entry_ids);
        assert_eq!(hash_all(folder.path()), before);
    }

    #[test]
    fn history_survives_a_restart_and_old_edits_stop_being_recoverable() {
        let folder = tempfile::tempdir().unwrap();
        crate::index::tests::copy_fixtures(folder.path());
        let data = tempfile::tempdir().unwrap();
        let database = data.path().join("folio.sqlite");
        let (root, plan_ids) = {
            let mut conn = crate::db::open(&database).unwrap();
            let root = crate::index::tests::authorize(&conn, folder.path());
            scan(&mut conn, &root);
            let first = { let operations = vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")]; apply_with(&mut conn, &root, operations, &RealFileSystem) };
            conn.execute("UPDATE action_plans SET applied_at = '1' WHERE id = ?1", [&first.batch.plan_id]).unwrap();
            conn.execute("UPDATE history SET applied_at = '1' WHERE plan_id = ?1", [&first.batch.plan_id]).unwrap();
            let second = { let operations = vec![relocate(&conn, &root, "notes/paalala.md", "archive/paalala.md", false)]; apply_with(&mut conn, &root, operations, &RealFileSystem) };
            (root, vec![first.batch.plan_id, second.batch.plan_id])
        };
        let mut conn = crate::db::open(&database).unwrap();
        prune_history(&conn, &root.id, 0).unwrap();
        let history = list_history(&conn, &root.id, 10).unwrap();
        assert_eq!(history.iter().map(|entry| (entry.plan_id.as_str(), entry.recoverable)).collect::<Vec<_>>(), vec![(plan_ids[1].as_str(), true), (plan_ids[0].as_str(), false)], "a move needs no stored content");
        assert_eq!(undo_all(&mut conn, &root, &plan_ids[0], &RealFileSystem).unwrap_err().code, ErrorCode::UndoConflict);
        undo_all(&mut conn, &root, &plan_ids[1], &RealFileSystem).unwrap();
        assert!(folder.path().join("notes/paalala.md").is_file());
        assert_eq!(list_history(&conn, &root.id, 1).unwrap().len(), 1, "listing is bounded");
    }

    #[test]
    fn passage_edits_need_one_exact_match_in_an_editable_file() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let plan = id_of(&root, "projects/project-plan.md");
        let reason = |result: NativeResult<FileOperation>| result.unwrap_err().detail("reason").map(str::to_owned);
        assert_eq!(reason(passage_edit(&conn, &root, &plan, "presentation", "talk")).as_deref(), Some("passageAmbiguous"));
        assert_eq!(reason(passage_edit(&conn, &root, &plan, "November 9", "November 10")).as_deref(), Some("passageNotFound"));
        assert_eq!(passage_edit(&conn, &root, &id_of(&root, "research/consent-form-guide.pdf"), "consent", "pahintulot").unwrap_err().code, ErrorCode::UnsupportedMediaType);
        assert_eq!(passage_edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23").unwrap_err().code, ErrorCode::DocumentUnavailable, "a path is not an identity");
    }

    #[test]
    fn a_stored_plan_keeps_no_file_bodies() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let report = { let operations = vec![create("notes/secret-body.md", "a body that should not be copied into plan storage")]; apply_with(&mut conn, &root, operations, &RealFileSystem) };
        let stored: String = conn.query_row("SELECT plan_json FROM action_plans WHERE id = ?1", [&report.batch.plan_id], |row| row.get(0)).unwrap();
        assert!(!stored.contains("should not be copied"));
    }
}
