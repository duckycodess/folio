use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use crate::error::{fail, ErrorCode, NativeError, NativeResult};
use crate::extract::{MediaKind, MAX_TEXT_BYTES};
use crate::index::{self, sha256_hex, IndexedDocument};
use crate::ripple::{self, ImpactCandidate};
use crate::workspace::{self, ScopedRoot};

pub const PLAN_LIFETIME_MS: i64 = 10 * 60 * 1000;
const MAX_OPERATIONS: usize = 20;
const RECOVERABLE_PLANS: usize = 100;
const CREATE_EXCERPT_CHARS: usize = 400;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as i64).unwrap_or(0)
}

/// A requested change. Operations come only from the caller (the user's request as
/// interpreted by the UI or provider); text inside documents can never create one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FileOperation {
    /// Replaces the single occurrence of `before` with `after` in a TXT/Markdown document.
    #[serde(rename_all = "camelCase")]
    Edit { document_id: String, expected_content_hash: String, before: String, after: String },
    /// New name in the same folder.
    #[serde(rename_all = "camelCase")]
    Rename { document_id: String, expected_content_hash: String, destination_relative_path: String },
    /// New location in an existing folder.
    #[serde(rename_all = "camelCase")]
    Move { document_id: String, expected_content_hash: String, destination_relative_path: String },
    #[serde(rename_all = "camelCase")]
    Create { destination_relative_path: String, content: String },
}

/// What the native core will do for one operation, bound to exact paths and hashes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedOperation {
    pub kind: String,
    pub document_id: Option<String>,
    pub source_path: Option<String>,
    pub destination_path: String,
    pub expected_hash: Option<String>,
    pub result_hash: String,
    pub content: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OperationDiff {
    pub kind: String,
    pub document_id: Option<String>,
    pub source_path: Option<String>,
    pub destination_path: String,
    /// The affected line before and after an edit; the opening of a created file.
    pub before_excerpt: Option<String>,
    pub after_excerpt: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StoredPlan {
    operations: Vec<FileOperation>,
    resolved: Vec<ResolvedOperation>,
    diffs: Vec<OperationDiff>,
    impacts: Vec<ImpactCandidate>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreview {
    pub id: String,
    pub workspace_id: String,
    pub operations: Vec<FileOperation>,
    pub impacts: Vec<ImpactCandidate>,
    pub diffs: Vec<OperationDiff>,
    pub created_at: i64,
    pub expires_at: i64,
    /// Approval must echo this digest; any change to the plan changes it.
    pub digest: String,
    pub status: String,
    pub status_message: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RevertReport {
    pub operation_index: usize,
    pub path: String,
    pub reverted: bool,
    pub message: Option<String>,
}

/// `status` is `applied` or `failed`. A failed apply lists which earlier writes were
/// reverted; no write is claimed as atomic.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub plan_id: String,
    pub status: String,
    pub message: Option<String>,
    pub failed_operation_index: Option<usize>,
    pub reverts: Vec<RevertReport>,
    pub index_refreshed: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UndoResult {
    pub plan_id: String,
    /// `undone`, or `partial` when a write failed midway (see `undoneOperations`).
    pub status: String,
    pub message: Option<String>,
    pub undone_operations: Vec<usize>,
    pub index_refreshed: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOperation {
    pub index: usize,
    pub kind: String,
    pub document_id: Option<String>,
    pub before_path: Option<String>,
    pub after_path: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub undone_at: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub plan_id: String,
    pub applied_at: Option<String>,
    pub undoable: bool,
    pub undone: bool,
    pub operations: Vec<HistoryOperation>,
}

/// File writes behind a trait so tests can inject failures.
pub trait FileSystem {
    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn create_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn remove(&self, path: &Path) -> io::Result<()>;
}

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    /// Writes a hidden temporary file beside the target, then renames it over the target.
    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let temporary = path.with_file_name(format!(".{name}.folio-{}.tmp", uuid::Uuid::new_v4()));
        let written = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if written.is_err() { let _ = fs::remove_file(&temporary); }
        written
    }

    fn create_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        let written = file.write_all(bytes).and_then(|_| file.sync_all());
        if written.is_err() {
            drop(file);
            let _ = fs::remove_file(path);
        }
        written
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "the destination already exists"));
        }
        fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}

// ---------------------------------------------------------------- validation

struct Target {
    document: IndexedDocument,
    bytes: Vec<u8>,
}

/// The indexed document, verified to be current on disk and to match the caller's expected hash.
fn current_target(conn: &Connection, root: &ScopedRoot, document_id: &str, expected_hash: &str) -> NativeResult<Target> {
    let document = index::get_document(conn, &root.id, document_id).map_err(|_| fail(ErrorCode::NotFound, "That document isn't in this folder's index. Choose the file before planning a change."))?;
    if document.status == "stale" || document.status == "failed" {
        return Err(fail(ErrorCode::StaleIndex, format!("{} changed since it was indexed. Refresh the folder before planning a change.", document.relative_path)));
    }
    let path = workspace::resolve_document(&root.path, &document.relative_path).map_err(|error| match error.code {
        ErrorCode::NotFound => fail(ErrorCode::TargetChanged, format!("{} is no longer in the folder.", document.relative_path)),
        _ => error,
    })?;
    let kind = MediaKind::from_path(&path).ok_or_else(|| fail(ErrorCode::Unsupported, "Unsupported file type."))?;
    let bytes = workspace::read_bounded(&path, kind.max_bytes())?;
    let hash = sha256_hex(&bytes);
    if hash != expected_hash {
        return Err(fail(ErrorCode::TargetChanged, format!("{} is not the version this change was planned against. Preview the change again.", document.relative_path)));
    }
    if hash != document.content_hash {
        return Err(fail(ErrorCode::StaleIndex, format!("{} changed since it was indexed. Refresh the folder before planning a change.", document.relative_path)));
    }
    Ok(Target { document, bytes })
}

/// A not-yet-existing destination inside the root whose folder already exists.
fn validate_destination(root: &Path, relative: &str) -> NativeResult<(String, PathBuf)> {
    if relative.trim().is_empty() {
        return Err(fail(ErrorCode::InvalidInput, "A destination path is required."));
    }
    if relative.contains('\\') || relative.contains(':') {
        return Err(fail(ErrorCode::PathEscape, "Use a relative path with forward slashes inside the folder."));
    }
    let path = Path::new(relative);
    if path.is_absolute() || relative.starts_with('/') || path.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(fail(ErrorCode::PathEscape, "The destination must stay inside the authorized folder."));
    }
    let normalized = path.components().map(|component| component.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if name.starts_with('.') {
        return Err(fail(ErrorCode::InvalidInput, "Hidden file names are not supported."));
    }
    MediaKind::from_path(path).ok_or_else(|| fail(ErrorCode::Unsupported, "The destination must be a TXT, Markdown or PDF file name."))?;
    let root = workspace::available_root(root)?;
    let parent = root.join(path.parent().unwrap_or(Path::new("")));
    let parent = parent.canonicalize().map_err(|_| fail(ErrorCode::NotFound, "The destination folder doesn't exist. Folio doesn't create folders yet."))?;
    if !parent.starts_with(&root) {
        return Err(fail(ErrorCode::PathEscape, "The destination folder is outside the authorized folder."));
    }
    if !parent.is_dir() {
        return Err(fail(ErrorCode::NotFound, "The destination folder doesn't exist."));
    }
    let target = parent.join(&name);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(fail(ErrorCode::Collision, format!("{normalized} already exists. Choose another name.")));
    }
    Ok((normalized, target))
}

fn extension(path: &str) -> String {
    Path::new(path).extension().map(|extension| extension.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

/// The full line containing `range` in `text`, without its line ending (LF or CRLF).
fn line_around(text: &str, start: usize, end: usize) -> (usize, usize) {
    let from = text[..start].rfind('\n').map_or(0, |newline| newline + 1);
    let to = text[end..].find('\n').map_or(text.len(), |newline| end + newline);
    let to = if to > end && text[..to].ends_with('\r') { to - 1 } else { to };
    (from, to)
}

// ---------------------------------------------------------------- plan

fn plan_digest(id: &str, workspace_id: &str, resolved: &[ResolvedOperation], created_at: i64, expires_at: i64) -> NativeResult<String> {
    Ok(sha256_hex(serde_json::to_string(&(id, workspace_id, resolved, created_at, expires_at))?.as_bytes()))
}

/// Validates and stores an exact, expiring plan. Nothing is written to user files.
pub fn create_plan(conn: &Connection, root: &ScopedRoot, operations: Vec<FileOperation>, now: i64) -> NativeResult<PlanPreview> {
    if operations.is_empty() {
        return Err(fail(ErrorCode::InvalidInput, "A plan needs at least one operation."));
    }
    if operations.len() > MAX_OPERATIONS {
        return Err(fail(ErrorCode::InvalidInput, format!("A plan can contain at most {MAX_OPERATIONS} operations.")));
    }
    let mut resolved = Vec::new();
    let mut diffs = Vec::new();
    let mut impacts: Vec<ImpactCandidate> = Vec::new();
    let mut documents = HashSet::new();
    let mut destinations = HashSet::new();
    for operation in &operations {
        let (step, diff) = match operation {
            FileOperation::Edit { document_id, expected_content_hash, before, after } => {
                let target = current_target(conn, root, document_id, expected_content_hash)?;
                if target.document.media_type == "application/pdf" {
                    return Err(fail(ErrorCode::UnsupportedEdit, "PDF content is read/index-only. Folio edits TXT and Markdown files."));
                }
                let text = String::from_utf8(target.bytes).map_err(|_| fail(ErrorCode::UnsupportedEdit, "Only UTF-8 text can be edited."))?;
                if before.is_empty() || before == after {
                    return Err(fail(ErrorCode::InvalidInput, "An edit needs the existing text and different replacement text."));
                }
                let path = &target.document.relative_path;
                match text.matches(before.as_str()).count() {
                    0 => return Err(fail(ErrorCode::AmbiguousEdit, format!("\u{201c}{before}\u{201d} was not found in {path}."))),
                    1 => {}
                    count => return Err(fail(ErrorCode::AmbiguousEdit, format!("\u{201c}{before}\u{201d} appears {count} times in {path}. Include more surrounding text."))),
                }
                let start = text.find(before.as_str()).unwrap_or(0);
                let content = text.replacen(before.as_str(), after, 1);
                if content.len() as u64 > MAX_TEXT_BYTES {
                    return Err(fail(ErrorCode::TooLarge, "The edited file would exceed the 2 MiB text limit."));
                }
                let (from, to) = line_around(&text, start, start + before.len());
                let after_line = format!("{}{}{}", &text[from..start], after, &text[start + before.len()..to]);
                for impact in ripple::impacts(conn, &root.id, &target.document, before)? {
                    if !impacts.iter().any(|existing| existing.document_id == impact.document_id) { impacts.push(impact); }
                }
                (
                    ResolvedOperation { kind: "edit".into(), document_id: Some(document_id.clone()), source_path: Some(path.clone()), destination_path: path.clone(), expected_hash: Some(expected_content_hash.clone()), result_hash: sha256_hex(content.as_bytes()), content: Some(content) },
                    OperationDiff { kind: "edit".into(), document_id: Some(document_id.clone()), source_path: Some(path.clone()), destination_path: path.clone(), before_excerpt: Some(text[from..to].to_owned()), after_excerpt: Some(after_line) },
                )
            }
            FileOperation::Rename { document_id, expected_content_hash, destination_relative_path } | FileOperation::Move { document_id, expected_content_hash, destination_relative_path } => {
                let kind = if matches!(operation, FileOperation::Rename { .. }) { "rename" } else { "move" };
                let target = current_target(conn, root, document_id, expected_content_hash)?;
                let (destination, _) = validate_destination(&root.path, destination_relative_path)?;
                let source = target.document.relative_path.clone();
                if extension(&destination) != extension(&source) {
                    return Err(fail(ErrorCode::InvalidInput, "Renaming or moving keeps the file's extension."));
                }
                if kind == "rename" && parent_of(&destination) != parent_of(&source) {
                    return Err(fail(ErrorCode::InvalidInput, "A rename stays in the same folder; use a move to change folders."));
                }
                (
                    ResolvedOperation { kind: kind.into(), document_id: Some(document_id.clone()), source_path: Some(source.clone()), destination_path: destination.clone(), expected_hash: Some(expected_content_hash.clone()), result_hash: expected_content_hash.clone(), content: None },
                    OperationDiff { kind: kind.into(), document_id: Some(document_id.clone()), source_path: Some(source), destination_path: destination, before_excerpt: None, after_excerpt: None },
                )
            }
            FileOperation::Create { destination_relative_path, content } => {
                let (destination, _) = validate_destination(&root.path, destination_relative_path)?;
                if !matches!(extension(&destination).as_str(), "md" | "txt") {
                    return Err(fail(ErrorCode::UnsupportedEdit, "Folio creates TXT and Markdown files only."));
                }
                if content.len() as u64 > MAX_TEXT_BYTES {
                    return Err(fail(ErrorCode::TooLarge, "New files are limited to 2 MiB."));
                }
                let excerpt: String = content.chars().take(CREATE_EXCERPT_CHARS).collect();
                (
                    ResolvedOperation { kind: "create".into(), document_id: None, source_path: None, destination_path: destination.clone(), expected_hash: None, result_hash: sha256_hex(content.as_bytes()), content: Some(content.clone()) },
                    OperationDiff { kind: "create".into(), document_id: None, source_path: None, destination_path: destination, before_excerpt: None, after_excerpt: Some(excerpt) },
                )
            }
        };
        if let Some(document_id) = &step.document_id {
            if !documents.insert(document_id.clone()) {
                return Err(fail(ErrorCode::InvalidInput, "A plan can change each document only once."));
            }
        }
        if step.kind != "edit" && !destinations.insert(step.destination_path.to_lowercase()) {
            return Err(fail(ErrorCode::Collision, format!("Two operations target {}.", step.destination_path)));
        }
        resolved.push(step);
        diffs.push(diff);
    }
    impacts.retain(|impact| !documents.contains(&impact.document_id));

    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = now + PLAN_LIFETIME_MS;
    let digest = plan_digest(&id, &root.id, &resolved, now, expires_at)?;
    let stored = StoredPlan { operations, resolved, diffs, impacts };
    conn.execute(
        "INSERT INTO action_plans (id, workspace_id, plan_json, plan_digest, status, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, 'preview', ?5, ?6)",
        params![id, root.id, serde_json::to_string(&stored)?, digest, now.to_string(), expires_at.to_string()],
    )?;
    Ok(PlanPreview { id, workspace_id: root.id.clone(), operations: stored.operations, impacts: stored.impacts, diffs: stored.diffs, created_at: now, expires_at, digest, status: "preview".into(), status_message: None })
}

struct PlanRow {
    id: String,
    workspace_id: String,
    stored: StoredPlan,
    digest: String,
    status: String,
    status_message: Option<String>,
    created_at: i64,
    expires_at: i64,
}

impl PlanRow {
    fn preview(self) -> PlanPreview {
        PlanPreview { id: self.id, workspace_id: self.workspace_id, operations: self.stored.operations, impacts: self.stored.impacts, diffs: self.stored.diffs, created_at: self.created_at, expires_at: self.expires_at, digest: self.digest, status: self.status, status_message: self.status_message }
    }

    /// The stored operations still hash to the digest the user saw.
    fn intact(&self) -> NativeResult<bool> {
        Ok(plan_digest(&self.id, &self.workspace_id, &self.stored.resolved, self.created_at, self.expires_at)? == self.digest)
    }
}

fn load_plan(conn: &Connection, workspace_id: &str, plan_id: &str) -> NativeResult<PlanRow> {
    let row = conn
        .query_row(
            "SELECT id, workspace_id, plan_json, plan_digest, status, status_message, created_at, expires_at FROM action_plans WHERE id = ?1 AND workspace_id = ?2",
            [plan_id, workspace_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, String>(6)?, row.get::<_, String>(7)?)),
        )
        .optional()?
        .ok_or_else(|| fail(ErrorCode::NotFound, "That plan doesn't exist in this workspace."))?;
    let (id, workspace_id, json, digest, status, status_message, created_at, expires_at) = row;
    let stored: StoredPlan = serde_json::from_str(&json).map_err(|_| fail(ErrorCode::PlanChanged, "The stored plan is unreadable. Preview the change again."))?;
    Ok(PlanRow { id, workspace_id, stored, digest, status, status_message, created_at: created_at.parse().unwrap_or(0), expires_at: expires_at.parse().unwrap_or(0) })
}

fn set_status(conn: &Connection, plan_id: &str, status: &str, message: Option<&str>) -> NativeResult<()> {
    conn.execute("UPDATE action_plans SET status = ?1, status_message = ?2 WHERE id = ?3", params![status, message, plan_id])?;
    Ok(())
}

fn expire(conn: &Connection, plan_id: &str, error: NativeError) -> NativeError {
    let _ = set_status(conn, plan_id, "expired", Some(&error.message));
    error
}

fn refuse_state(status: &str) -> NativeError {
    match status {
        "expired" => fail(ErrorCode::PlanExpired, "This preview expired or its files changed. Preview the change again."),
        "preview" => fail(ErrorCode::ApprovalRequired, "Approve this exact preview before applying it."),
        other => fail(ErrorCode::PlanState, format!("This plan is already {other}.")),
    }
}

/// Records the user's approval of exactly this preview. The digest must match the stored plan.
pub fn approve_plan(conn: &Connection, workspace_id: &str, plan_id: &str, digest: &str, now: i64) -> NativeResult<PlanPreview> {
    let mut plan = load_plan(conn, workspace_id, plan_id)?;
    if plan.status != "preview" {
        return Err(refuse_state(&plan.status));
    }
    if now >= plan.expires_at || now < plan.created_at {
        return Err(expire(conn, plan_id, fail(ErrorCode::PlanExpired, "This preview expired. Preview the change again.")));
    }
    if digest != plan.digest || !plan.intact()? {
        return Err(fail(ErrorCode::PlanChanged, "The approved plan does not match the stored preview. Preview the change again."));
    }
    conn.execute("INSERT INTO approvals (plan_id, plan_digest, approved_at) VALUES (?1, ?2, ?3)", params![plan_id, plan.digest, now.to_string()])?;
    set_status(conn, plan_id, "approved", None)?;
    plan.status = "approved".into();
    Ok(plan.preview())
}

enum Revert {
    Restore { path: PathBuf, bytes: Vec<u8>, written_hash: String },
    MoveBack { from: PathBuf, to: PathBuf },
    Remove { path: PathBuf, written_hash: String },
}

fn hash_of(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// Reverses a completed write only if the file is still exactly what Folio wrote.
fn revert(files: &dyn FileSystem, step: &Revert) -> Result<(), String> {
    match step {
        Revert::Restore { path, bytes, written_hash } => {
            if hash_of(path).as_ref() != Some(written_hash) { return Err("the file changed again; left as is".into()); }
            files.replace(path, bytes).map_err(|error| error.to_string())
        }
        Revert::MoveBack { from, to } => files.rename(from, to).map_err(|error| error.to_string()),
        Revert::Remove { path, written_hash } => {
            if hash_of(path).as_ref() != Some(written_hash) { return Err("the file changed again; left as is".into()); }
            files.remove(path).map_err(|error| error.to_string())
        }
    }
}

fn display(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// Applies an approved, unexpired plan after re-checking every target. On a write failure,
/// earlier writes are reverted where possible and the result says exactly what happened;
/// the index is refreshed only after every write succeeded.
pub fn apply_plan(conn: &mut Connection, root: &ScopedRoot, plan_id: &str, now: i64, files: &dyn FileSystem) -> NativeResult<ApplyResult> {
    let plan = load_plan(conn, &root.id, plan_id)?;
    if plan.status != "approved" {
        return Err(refuse_state(&plan.status));
    }
    let approved: Option<String> = conn.query_row("SELECT plan_digest FROM approvals WHERE plan_id = ?1", [plan_id], |row| row.get(0)).optional()?;
    if approved.as_deref() != Some(plan.digest.as_str()) || !plan.intact()? {
        return Err(fail(ErrorCode::PlanChanged, "The plan no longer matches what was approved. Preview the change again."));
    }
    if now >= plan.expires_at {
        return Err(expire(conn, plan_id, fail(ErrorCode::PlanExpired, "This approval expired. Preview the change again.")));
    }
    let root_path = workspace::available_root(&root.path)?;

    // Re-check every target before the first write.
    let mut originals: Vec<Option<(PathBuf, Vec<u8>)>> = Vec::new();
    let mut destinations: Vec<PathBuf> = Vec::new();
    for step in &plan.stored.resolved {
        let source = match &step.source_path {
            Some(source) => {
                let path = workspace::resolve_document(&root_path, source).map_err(|_| expire(conn, plan_id, fail(ErrorCode::TargetChanged, format!("{source} is no longer in the folder. Preview the change again."))))?;
                let bytes = workspace::read_bounded(&path, MediaKind::from_path(&path).map_or(MAX_TEXT_BYTES, MediaKind::max_bytes))?;
                if step.expected_hash.as_deref() != Some(sha256_hex(&bytes).as_str()) {
                    return Err(expire(conn, plan_id, fail(ErrorCode::TargetChanged, format!("{source} changed after the preview. Preview the change again."))));
                }
                Some((path, bytes))
            }
            None => None,
        };
        let destination = match (&step.kind[..], &source) {
            ("edit", Some((path, _))) => path.clone(),
            _ => validate_destination(&root_path, &step.destination_path).map_err(|error| expire(conn, plan_id, error))?.1,
        };
        originals.push(source);
        destinations.push(destination);
    }

    let mut completed: Vec<(usize, Revert)> = Vec::new();
    for (index, step) in plan.stored.resolved.iter().enumerate() {
        let destination = &destinations[index];
        let written = match &step.kind[..] {
            "edit" => files.replace(destination, step.content.as_deref().unwrap_or("").as_bytes()).map(|_| {
                let (path, bytes) = originals[index].clone().expect("edit has a source");
                Revert::Restore { path, bytes, written_hash: step.result_hash.clone() }
            }),
            "rename" | "move" => {
                let (source, _) = originals[index].as_ref().expect("relocation has a source");
                files.rename(source, destination).map(|_| Revert::MoveBack { from: destination.clone(), to: source.clone() })
            }
            _ => files.create_new(destination, step.content.as_deref().unwrap_or("").as_bytes()).map(|_| Revert::Remove { path: destination.clone(), written_hash: step.result_hash.clone() }),
        };
        match written {
            Ok(revert_step) => completed.push((index, revert_step)),
            Err(error) => {
                let mut reverts = Vec::new();
                for (done, step) in completed.iter().rev() {
                    let path = display(&destinations[*done], &root_path);
                    match revert(files, step) {
                        Ok(()) => reverts.push(RevertReport { operation_index: *done, path, reverted: true, message: None }),
                        Err(message) => reverts.push(RevertReport { operation_index: *done, path, reverted: false, message: Some(message) }),
                    }
                }
                let unrecovered = reverts.iter().filter(|report| !report.reverted).count();
                let message = format!(
                    "Saving {} failed: {error}. {} earlier change(s) reverted{}.",
                    step.destination_path,
                    reverts.len() - unrecovered,
                    if unrecovered > 0 { format!(", {unrecovered} could not be reverted and need review") } else { String::new() }
                );
                set_status(conn, plan_id, "failed", Some(&message))?;
                return Ok(ApplyResult { plan_id: plan_id.into(), status: "failed".into(), message: Some(message), failed_operation_index: Some(index), reverts, index_refreshed: false });
            }
        }
    }

    let recorded = record_history(conn, root, &plan, &originals, now);
    if let Err(error) = recorded {
        // Files changed but no history could be stored: undo the writes rather than leave unrecoverable changes.
        let reverts: Vec<RevertReport> = completed
            .iter()
            .rev()
            .map(|(index, step)| {
                let outcome = revert(files, step);
                RevertReport { operation_index: *index, path: display(&destinations[*index], &root_path), reverted: outcome.is_ok(), message: outcome.err() }
            })
            .collect();
        let message = format!("History could not be recorded ({}); the changes were reverted where possible.", error.message);
        let _ = set_status(conn, plan_id, "failed", Some(&message));
        return Ok(ApplyResult { plan_id: plan_id.into(), status: "failed".into(), message: Some(message), failed_operation_index: None, reverts, index_refreshed: false });
    }

    let mut affected: Vec<String> = Vec::new();
    for step in &plan.stored.resolved {
        affected.extend(step.source_path.iter().cloned());
        affected.push(step.destination_path.clone());
    }
    affected.dedup();
    let refreshed = index::refresh_paths(conn, root, &affected);
    if refreshed.is_ok() {
        conn.execute(
            "UPDATE history SET document_id = (SELECT id FROM documents WHERE workspace_id = ?1 AND relative_path = history.after_path) WHERE plan_id = ?2 AND document_id IS NULL",
            params![root.id, plan_id],
        )?;
    }
    Ok(ApplyResult {
        plan_id: plan_id.into(),
        status: "applied".into(),
        message: refreshed.as_ref().err().map(|error| format!("Saved, but the index could not be refreshed: {}. Refresh the folder.", error.message)),
        failed_operation_index: None,
        reverts: Vec::new(),
        index_refreshed: refreshed.is_ok(),
    })
}

fn record_history(conn: &mut Connection, root: &ScopedRoot, plan: &PlanRow, originals: &[Option<(PathBuf, Vec<u8>)>], now: i64) -> NativeResult<()> {
    let tx = conn.transaction()?;
    for (index, step) in plan.stored.resolved.iter().enumerate() {
        let before_content = (step.kind == "edit").then(|| originals[index].as_ref().map(|(_, bytes)| bytes.clone())).flatten();
        if step.kind == "rename" || step.kind == "move" {
            // A stale index row at the destination would collide with the moved document's path.
            let stale: Option<String> = tx.query_row("SELECT id FROM documents WHERE workspace_id = ?1 AND relative_path = ?2", [&root.id, &step.destination_path], |row| row.get(0)).optional()?;
            if let Some(stale) = stale { index::forget_document(&tx, &stale)?; }
            let name = step.destination_path.rsplit('/').next().unwrap_or(&step.destination_path);
            tx.execute("UPDATE documents SET relative_path = ?1, name = ?2 WHERE id = ?3", params![step.destination_path, name, step.document_id])?;
        }
        tx.execute(
            "INSERT INTO history (id, plan_id, document_id, before_path, after_path, before_content, after_hash, applied_at, operation_index, operation_kind, before_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![uuid::Uuid::new_v4().to_string(), plan.id, step.document_id, step.source_path, step.destination_path, before_content, step.result_hash, now.to_string(), index as i64, step.kind, step.expected_hash],
        )?;
    }
    tx.execute("UPDATE action_plans SET status = 'applied', status_message = NULL, applied_at = ?1 WHERE id = ?2", params![now.to_string(), plan.id])?;
    prune_history(&tx, &root.id, RECOVERABLE_PLANS)?;
    tx.commit()?;
    Ok(())
}

/// Keeps recoverable content for the most recent `keep` applied plans; older ones stay listed but can't be undone.
fn prune_history(conn: &Connection, workspace_id: &str, keep: usize) -> NativeResult<()> {
    conn.execute(
        "UPDATE history SET before_content = NULL, content_pruned = 1 WHERE content_pruned = 0 AND plan_id IN (SELECT id FROM action_plans WHERE workspace_id = ?1 AND status = 'applied' ORDER BY CAST(applied_at AS INTEGER) DESC, rowid DESC LIMIT -1 OFFSET ?2)",
        params![workspace_id, keep as i64],
    )?;
    Ok(())
}

struct HistoryRow {
    id: String,
    index: usize,
    kind: String,
    before_path: Option<String>,
    after_path: Option<String>,
    before_content: Option<Vec<u8>>,
    after_hash: Option<String>,
    undone_at: Option<String>,
    pruned: bool,
}

fn history_rows(conn: &Connection, plan_id: &str) -> NativeResult<Vec<HistoryRow>> {
    let mut statement = conn.prepare("SELECT id, operation_index, operation_kind, before_path, after_path, before_content, after_hash, undone_at, content_pruned FROM history WHERE plan_id = ?1 ORDER BY operation_index")?;
    let rows = statement.query_map([plan_id], |row| {
        Ok(HistoryRow {
            id: row.get(0)?,
            index: row.get::<_, i64>(1)? as usize,
            kind: row.get(2)?,
            before_path: row.get(3)?,
            after_path: row.get(4)?,
            before_content: row.get(5)?,
            after_hash: row.get(6)?,
            undone_at: row.get(7)?,
            pruned: row.get::<_, i64>(8)? != 0,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Reverses a whole applied plan after checking that every file is still exactly what Folio
/// wrote. If anything changed since, nothing is touched and UNDO_CONFLICT is returned.
pub fn undo_plan(conn: &mut Connection, root: &ScopedRoot, plan_id: &str, now: i64, files: &dyn FileSystem) -> NativeResult<UndoResult> {
    let plan = load_plan(conn, &root.id, plan_id)?;
    if plan.status != "applied" {
        return Err(fail(ErrorCode::PlanState, format!("Only applied changes can be undone; this plan is {}.", plan.status)));
    }
    let rows = history_rows(conn, plan_id)?;
    if rows.is_empty() || rows.iter().any(|row| row.undone_at.is_some()) {
        return Err(fail(ErrorCode::PlanState, "This change was already undone."));
    }
    if rows.iter().any(|row| row.pruned) {
        return Err(fail(ErrorCode::UndoUnavailable, "This change is too old to undo; its previous content is no longer kept."));
    }
    let root_path = workspace::available_root(&root.path)?;
    let conflict = |path: &str, detail: &str| fail(ErrorCode::UndoConflict, format!("{path} {detail} Undo would overwrite those changes, so nothing was undone."));

    let mut steps: Vec<(&HistoryRow, PathBuf, Option<PathBuf>)> = Vec::new();
    for row in rows.iter().rev() {
        let after = row.after_path.as_deref().unwrap_or_default();
        let current = workspace::resolve_document(&root_path, after).map_err(|_| conflict(after, "is no longer where Folio saved it."))?;
        if hash_of(&current) != row.after_hash {
            return Err(conflict(after, "changed after Folio saved it."));
        }
        let restore_to = match &row.kind[..] {
            "rename" | "move" => {
                let before = row.before_path.as_deref().unwrap_or_default();
                Some(validate_destination(&root_path, before).map_err(|_| conflict(before, "is occupied or its folder is gone."))?.1)
            }
            _ => None,
        };
        steps.push((row, current, restore_to));
    }

    let mut undone = Vec::new();
    let mut failure = None;
    for (row, current, restore_to) in &steps {
        let result = match &row.kind[..] {
            "edit" => files.replace(current, row.before_content.as_deref().unwrap_or_default()),
            "rename" | "move" => files.rename(current, restore_to.as_ref().expect("relocation has an origin")),
            _ => files.remove(current),
        };
        match result {
            Ok(()) => undone.push(*row),
            Err(error) => {
                failure = Some(format!("Undoing {} failed: {error}.", row.after_path.as_deref().unwrap_or_default()));
                break;
            }
        }
    }

    let tx = conn.transaction()?;
    for row in &undone {
        tx.execute("UPDATE history SET undone_at = ?1 WHERE id = ?2", params![now.to_string(), row.id])?;
        if row.kind == "rename" || row.kind == "move" {
            let before = row.before_path.as_deref().unwrap_or_default();
            let name = before.rsplit('/').next().unwrap_or(before);
            tx.execute("UPDATE documents SET relative_path = ?1, name = ?2 WHERE workspace_id = ?3 AND relative_path = ?4", params![before, name, root.id, row.after_path])?;
        }
    }
    tx.commit()?;
    let mut affected: Vec<String> = undone.iter().flat_map(|row| row.before_path.iter().chain(row.after_path.iter()).cloned()).collect();
    affected.dedup();
    let refreshed = index::refresh_paths(conn, root, &affected).is_ok();
    let undone_operations = undone.iter().map(|row| row.index).collect();
    Ok(match failure {
        Some(message) => UndoResult { plan_id: plan_id.into(), status: "partial".into(), message: Some(format!("{message} Earlier steps were undone; the rest are unchanged.")), undone_operations, index_refreshed: refreshed },
        None => UndoResult { plan_id: plan_id.into(), status: "undone".into(), message: (!refreshed).then(|| "Undone, but the index could not be refreshed. Refresh the folder.".into()), undone_operations, index_refreshed: refreshed },
    })
}

pub fn list_history(conn: &Connection, workspace_id: &str) -> NativeResult<Vec<HistoryEntry>> {
    let mut statement = conn.prepare("SELECT id, applied_at FROM action_plans WHERE workspace_id = ?1 AND status = 'applied' ORDER BY CAST(applied_at AS INTEGER) DESC, rowid DESC")?;
    let plans: Vec<(String, Option<String>)> = statement.query_map([workspace_id], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut entries = Vec::new();
    for (plan_id, applied_at) in plans {
        let mut statement = conn.prepare("SELECT operation_index, operation_kind, document_id, before_path, after_path, before_hash, after_hash, undone_at, content_pruned FROM history WHERE plan_id = ?1 ORDER BY operation_index")?;
        let mut pruned = false;
        let operations: Vec<HistoryOperation> = statement
            .query_map([&plan_id], |row| {
                Ok((
                    HistoryOperation {
                        index: row.get::<_, i64>(0)? as usize,
                        kind: row.get(1)?,
                        document_id: row.get(2)?,
                        before_path: row.get(3)?,
                        after_path: row.get(4)?,
                        before_hash: row.get(5)?,
                        after_hash: row.get(6)?,
                        undone_at: row.get(7)?,
                    },
                    row.get::<_, i64>(8)? != 0,
                ))
            })?
            .map(|row| row.map(|(operation, row_pruned)| {
                pruned |= row_pruned;
                operation
            }))
            .collect::<Result<_, _>>()?;
        let undone = !operations.is_empty() && operations.iter().all(|operation| operation.undone_at.is_some());
        entries.push(HistoryEntry { plan_id, applied_at, undoable: !undone && !pruned && operations.iter().all(|operation| operation.undone_at.is_none()), undone, operations });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::{fixture_workspace, id_of, scan};
    use std::cell::Cell;
    use std::collections::BTreeMap;

    const T0: i64 = 1_760_000_000_000;

    fn hash_all(folder: &Path) -> BTreeMap<String, String> {
        walkdir::WalkDir::new(folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| (display(entry.path(), folder), sha256_hex(&fs::read(entry.path()).unwrap())))
            .collect()
    }

    fn hash(conn: &Connection, root: &ScopedRoot, path: &str) -> String {
        index::get_document(conn, &root.id, &id_of(conn, root, path)).unwrap().content_hash
    }

    fn edit(conn: &Connection, root: &ScopedRoot, path: &str, before: &str, after: &str) -> FileOperation {
        FileOperation::Edit { document_id: id_of(conn, root, path), expected_content_hash: hash(conn, root, path), before: before.into(), after: after.into() }
    }

    fn relocate(conn: &Connection, root: &ScopedRoot, path: &str, destination: &str, rename: bool) -> FileOperation {
        let (document_id, expected_content_hash, destination_relative_path) = (id_of(conn, root, path), hash(conn, root, path), destination.to_owned());
        if rename { FileOperation::Rename { document_id, expected_content_hash, destination_relative_path } } else { FileOperation::Move { document_id, expected_content_hash, destination_relative_path } }
    }

    fn approve_and_apply(conn: &mut Connection, root: &ScopedRoot, operations: Vec<FileOperation>) -> (PlanPreview, ApplyResult) {
        let plan = create_plan(conn, root, operations, T0).unwrap();
        approve_plan(conn, &root.id, &plan.id, &plan.digest, T0 + 1).unwrap();
        let result = apply_plan(conn, root, &plan.id, T0 + 2, &RealFileSystem).unwrap();
        (plan, result)
    }

    fn indexed_text(conn: &Connection, root: &ScopedRoot, path: &str) -> String {
        conn.query_row("SELECT group_concat(chunk_text, '\n') FROM chunks WHERE document_id = ?1", [id_of(conn, root, path)], |row| row.get(0)).unwrap()
    }

    /// Fails the Nth write (1-based) and delegates everything else.
    struct FailingFileSystem {
        fail_on: usize,
        calls: Cell<usize>,
    }

    impl FailingFileSystem {
        fn tick(&self) -> io::Result<()> {
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == self.fail_on { Err(io::Error::other("injected write failure")) } else { Ok(()) }
        }
    }

    impl FileSystem for FailingFileSystem {
        fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> { self.tick()?; RealFileSystem.replace(path, bytes) }
        fn create_new(&self, path: &Path, bytes: &[u8]) -> io::Result<()> { self.tick()?; RealFileSystem.create_new(path, bytes) }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> { self.tick()?; RealFileSystem.rename(from, to) }
        fn remove(&self, path: &Path) -> io::Result<()> { self.tick()?; RealFileSystem.remove(path) }
    }

    #[test]
    fn ripple_deadline_case_flags_related_passages_and_changes_only_the_target() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let operation = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
        let plan = create_plan(&conn, &root, vec![operation], T0).unwrap();
        assert_eq!(hash_all(folder.path()), before, "previewing writes nothing");

        let evidence: Vec<&str> = plan.impacts.iter().filter(|impact| impact.strength == "evidence").map(|impact| impact.relative_path.as_str()).collect();
        assert_eq!(evidence, vec!["meetings/meeting-notes.md", "notes/tala-sa-proyekto.md", "projects/submission-checklist.md"]);
        for impact in plan.impacts.iter().filter(|impact| impact.strength == "evidence") {
            assert!(impact.evidence.iter().all(|passage| passage.text.contains("October 20")));
        }
        let similar: Vec<&str> = plan.impacts.iter().filter(|impact| impact.strength == "similarityOnly").map(|impact| impact.relative_path.as_str()).collect();
        assert_eq!(similar, vec!["archive/project-plan-copy.md"]);
        assert!(!plan.impacts.iter().any(|impact| impact.relative_path.starts_with("courses/")), "same date, unrelated event");
        assert_eq!(plan.diffs[0].before_excerpt.as_deref(), Some("The project submission deadline is October 20. The team will prepare a short presentation, a written report, and a demonstration."));
        assert!(plan.diffs[0].after_excerpt.as_deref().unwrap().contains("October 23."));

        approve_plan(&conn, &root.id, &plan.id, &plan.digest, T0 + 1).unwrap();
        let result = apply_plan(&mut conn, &root, &plan.id, T0 + 2, &RealFileSystem).unwrap();
        assert_eq!((result.status.as_str(), result.index_refreshed), ("applied", true));
        let after = hash_all(folder.path());
        let changed: Vec<&String> = after.keys().filter(|path| before.get(*path) != after.get(*path)).collect();
        assert_eq!(changed, vec!["projects/project-plan.md"]);
        assert!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap().contains("deadline is October 23."));
        let indexed = indexed_text(&conn, &root, "projects/project-plan.md");
        assert!(indexed.contains("October 23") && !indexed.contains("October 20"));
        let history = list_history(&conn, &root.id).unwrap();
        assert_eq!((history.len(), history[0].undoable), (1, true));
    }

    #[test]
    fn nothing_is_written_without_approval() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let plan = create_plan(&conn, &root, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")], T0).unwrap();
        assert_eq!(apply_plan(&mut conn, &root, &plan.id, T0 + 1, &RealFileSystem).unwrap_err().code, ErrorCode::ApprovalRequired);
        assert_eq!(apply_plan(&mut conn, &root, "made-up-plan", T0 + 1, &RealFileSystem).unwrap_err().code, ErrorCode::NotFound);
        assert_eq!(hash_all(folder.path()), before);
    }

    #[test]
    fn expired_plans_are_refused_at_approval_and_apply() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let late = create_plan(&conn, &root, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")], T0).unwrap();
        assert_eq!(approve_plan(&conn, &root.id, &late.id, &late.digest, T0 + PLAN_LIFETIME_MS).unwrap_err().code, ErrorCode::PlanExpired);
        let approved = create_plan(&conn, &root, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")], T0).unwrap();
        approve_plan(&conn, &root.id, &approved.id, &approved.digest, T0 + 1).unwrap();
        assert_eq!(apply_plan(&mut conn, &root, &approved.id, T0 + PLAN_LIFETIME_MS + 1, &RealFileSystem).unwrap_err().code, ErrorCode::PlanExpired);
        assert_eq!(apply_plan(&mut conn, &root, &approved.id, T0 + 2, &RealFileSystem).unwrap_err().code, ErrorCode::PlanExpired, "an expired plan stays expired");
        assert_eq!(hash_all(folder.path()), before);
    }

    #[test]
    fn changed_operations_are_refused() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let plan = create_plan(&conn, &root, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")], T0).unwrap();
        assert_eq!(approve_plan(&conn, &root.id, &plan.id, "ui-invented-token", T0 + 1).unwrap_err().code, ErrorCode::PlanChanged);
        approve_plan(&conn, &root.id, &plan.id, &plan.digest, T0 + 1).unwrap();
        let json: String = conn.query_row("SELECT plan_json FROM action_plans WHERE id = ?1", [&plan.id], |row| row.get(0)).unwrap();
        conn.execute("UPDATE action_plans SET plan_json = ?1 WHERE id = ?2", params![json.replace("October 23", "December 1"), plan.id]).unwrap();
        assert_eq!(apply_plan(&mut conn, &root, &plan.id, T0 + 2, &RealFileSystem).unwrap_err().code, ErrorCode::PlanChanged);
        assert_eq!(hash_all(folder.path()), before);
    }

    #[test]
    fn external_edit_after_approval_requires_a_new_preview() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let plan = create_plan(&conn, &root, vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")], T0).unwrap();
        approve_plan(&conn, &root.id, &plan.id, &plan.digest, T0 + 1).unwrap();
        let external = "# Community Learning Project\n\nThe project submission deadline is October 20. Edited elsewhere.\n";
        fs::write(folder.path().join("projects/project-plan.md"), external).unwrap();
        assert_eq!(apply_plan(&mut conn, &root, &plan.id, T0 + 2, &RealFileSystem).unwrap_err().code, ErrorCode::TargetChanged);
        assert_eq!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap(), external);
        assert_eq!(apply_plan(&mut conn, &root, &plan.id, T0 + 3, &RealFileSystem).unwrap_err().code, ErrorCode::PlanExpired);
    }

    #[test]
    fn unsafe_or_ambiguous_requests_are_refused_without_changes() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let refused = |operation: FileOperation| create_plan(&conn, &root, vec![operation], T0).unwrap_err().code;
        assert_eq!(refused(relocate(&conn, &root, "projects/project-plan.md", "projects/submission-checklist.md", true)), ErrorCode::Collision);
        assert_eq!(refused(relocate(&conn, &root, "projects/project-plan.md", "../escaped.md", false)), ErrorCode::PathEscape);
        let absolute = folder.path().join("elsewhere.md").to_string_lossy().into_owned();
        assert_eq!(refused(relocate(&conn, &root, "projects/project-plan.md", &absolute, false)), ErrorCode::PathEscape);
        assert_eq!(refused(relocate(&conn, &root, "projects/project-plan.md", "missing-folder/plan.md", false)), ErrorCode::NotFound);
        assert_eq!(refused(relocate(&conn, &root, "projects/project-plan.md", "notes/plan.md", true)), ErrorCode::InvalidInput, "rename keeps the folder");
        assert_eq!(refused(edit(&conn, &root, "research/consent-form-guide.pdf", "consent", "pahintulot")), ErrorCode::UnsupportedEdit);
        assert_eq!(refused(edit(&conn, &root, "projects/project-plan.md", "presentation", "talk")), ErrorCode::AmbiguousEdit);
        assert_eq!(refused(edit(&conn, &root, "projects/project-plan.md", "November 9", "November 10")), ErrorCode::AmbiguousEdit);
        assert_eq!(refused(FileOperation::Edit { document_id: "projects/project-plan.md".into(), expected_content_hash: String::new(), before: "October 20".into(), after: "October 23".into() }), ErrorCode::NotFound, "paths are not identities");
        assert_eq!(refused(FileOperation::Create { destination_relative_path: "notes/paalala.md".into(), content: "x".into() }), ErrorCode::Collision);
        assert_eq!(refused(FileOperation::Create { destination_relative_path: "notes/scan.pdf".into(), content: "x".into() }), ErrorCode::UnsupportedEdit);
        assert_eq!(hash_all(folder.path()), before);
    }

    #[test]
    fn symlinked_destination_folder_outside_the_root_is_refused() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let outside = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(outside.path(), folder.path().join("portal"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(outside.path(), folder.path().join("portal"));
        if linked.is_err() {
            eprintln!("skipping: this host does not permit creating symlinks");
            return;
        }
        let error = create_plan(&conn, &root, vec![relocate(&conn, &root, "projects/project-plan.md", "portal/plan.md", false)], T0).unwrap_err();
        assert_eq!(error.code, ErrorCode::PathEscape);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    #[test]
    fn stale_index_must_be_refreshed_before_planning() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let operation = edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23");
        fs::write(folder.path().join("projects/project-plan.md"), "# Plan\n\nNew text with October 20.\n").unwrap();
        assert_eq!(create_plan(&conn, &root, vec![operation], T0).unwrap_err().code, ErrorCode::TargetChanged);
        let current = sha256_hex(&fs::read(folder.path().join("projects/project-plan.md")).unwrap());
        let operation = FileOperation::Edit { document_id: id_of(&conn, &root, "projects/project-plan.md"), expected_content_hash: current, before: "October 20".into(), after: "October 23".into() };
        assert_eq!(create_plan(&conn, &root, vec![operation], T0).unwrap_err().code, ErrorCode::StaleIndex);
    }

    #[test]
    fn create_rename_and_move_save_real_files_record_history_and_refresh_the_index() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let checklist_id = id_of(&conn, &root, "projects/submission-checklist.md");
        let operations = vec![
            FileOperation::Create { destination_relative_path: "notes/bagong-tala.md".into(), content: "# Bagong tala\n\nPaalala tungkol sa xylophone practice.\n".into() },
            relocate(&conn, &root, "projects/submission-checklist.md", "projects/checklist.md", true),
            relocate(&conn, &root, "personal/grocery-list.md", "archive/grocery-list.md", false),
        ];
        let (plan, result) = approve_and_apply(&mut conn, &root, operations);
        assert_eq!(result.status, "applied");
        assert!(folder.path().join("notes/bagong-tala.md").is_file());
        assert!(folder.path().join("projects/checklist.md").is_file() && !folder.path().join("projects/submission-checklist.md").exists());
        assert!(folder.path().join("archive/grocery-list.md").is_file() && !folder.path().join("personal/grocery-list.md").exists());
        assert_eq!(id_of(&conn, &root, "projects/checklist.md"), checklist_id, "a rename keeps the document identity");
        let hits = index::search(&conn, &root.id, "xylophone", 5).unwrap();
        assert_eq!(hits[0].document.relative_path, "notes/bagong-tala.md");
        let history = list_history(&conn, &root.id).unwrap();
        assert_eq!(history[0].plan_id, plan.id);
        assert_eq!(history[0].operations.iter().map(|operation| operation.kind.as_str()).collect::<Vec<_>>(), vec!["create", "rename", "move"]);
        assert!(history[0].operations[0].document_id.is_some(), "created file is linked to its new index record");
        let rescan = scan(&mut conn, &root);
        assert_eq!(rescan.added + rescan.updated + rescan.removed, 0, "index already matched the disk");
    }

    #[test]
    fn failed_write_reverts_earlier_writes_and_leaves_the_index_alone() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let operations = vec![
            edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"),
            edit(&conn, &root, "meetings/meeting-notes.md", "October 20", "October 23"),
        ];
        let plan = create_plan(&conn, &root, operations, T0).unwrap();
        approve_plan(&conn, &root.id, &plan.id, &plan.digest, T0 + 1).unwrap();
        let result = apply_plan(&mut conn, &root, &plan.id, T0 + 2, &FailingFileSystem { fail_on: 2, calls: Cell::new(0) }).unwrap();
        assert_eq!((result.status.as_str(), result.failed_operation_index), ("failed", Some(1)));
        assert_eq!(result.reverts.len(), 1);
        assert!(result.reverts[0].reverted);
        assert!(result.message.unwrap().contains("1 earlier change(s) reverted"));
        assert_eq!(hash_all(folder.path()), before, "the first write was reverted and no temp files remain");
        assert!(indexed_text(&conn, &root, "projects/project-plan.md").contains("October 20"));
        assert!(list_history(&conn, &root.id).unwrap().is_empty());
        let status: String = conn.query_row("SELECT status FROM action_plans WHERE id = ?1", [&plan.id], |row| row.get(0)).unwrap();
        assert_eq!(status, "failed");
    }

    #[test]
    fn undo_restores_content_paths_and_index() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = hash_all(folder.path());
        let operations = vec![
            edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"),
            relocate(&conn, &root, "personal/grocery-list.md", "archive/grocery-list.md", false),
            FileOperation::Create { destination_relative_path: "notes/new.md".into(), content: "# New\n".into() },
        ];
        let (plan, _) = approve_and_apply(&mut conn, &root, operations);
        let undone = undo_plan(&mut conn, &root, &plan.id, T0 + 3, &RealFileSystem).unwrap();
        assert_eq!((undone.status.as_str(), undone.index_refreshed), ("undone", true));
        assert_eq!(hash_all(folder.path()), before);
        assert!(indexed_text(&conn, &root, "projects/project-plan.md").contains("October 20"));
        assert_eq!(index::list_documents(&conn, &root.id).unwrap().iter().filter(|document| document.relative_path == "notes/new.md").count(), 0);
        let history = list_history(&conn, &root.id).unwrap();
        assert!(history[0].undone && !history[0].undoable);
        assert_eq!(undo_plan(&mut conn, &root, &plan.id, T0 + 4, &RealFileSystem).unwrap_err().code, ErrorCode::PlanState);
    }

    #[test]
    fn undo_refuses_to_overwrite_external_edits() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let (plan, _) = {
            let operations = vec![
            edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23"),
            edit(&conn, &root, "meetings/meeting-notes.md", "October 20", "October 23"),
        ];
            approve_and_apply(&mut conn, &root, operations)
        };
        let external = fs::read_to_string(folder.path().join("meetings/meeting-notes.md")).unwrap() + "\nExternal note added after the save.\n";
        fs::write(folder.path().join("meetings/meeting-notes.md"), &external).unwrap();
        let error = undo_plan(&mut conn, &root, &plan.id, T0 + 3, &RealFileSystem).unwrap_err();
        assert_eq!(error.code, ErrorCode::UndoConflict);
        assert_eq!(fs::read_to_string(folder.path().join("meetings/meeting-notes.md")).unwrap(), external);
        assert!(fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap().contains("October 23"), "no partial undo");
    }

    #[test]
    fn undo_of_create_refuses_when_the_new_file_was_changed() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let (plan, _) = {
            let operations = vec![FileOperation::Create { destination_relative_path: "notes/new.md".into(), content: "# New\n".into() }];
            approve_and_apply(&mut conn, &root, operations)
        };
        fs::write(folder.path().join("notes/new.md"), "# New\n\nUser kept writing here.\n").unwrap();
        assert_eq!(undo_plan(&mut conn, &root, &plan.id, T0 + 3, &RealFileSystem).unwrap_err().code, ErrorCode::UndoConflict);
        assert!(folder.path().join("notes/new.md").is_file());
    }

    #[test]
    fn old_history_is_kept_listed_but_not_undoable() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let (first, _) = {
            let operations = vec![edit(&conn, &root, "projects/project-plan.md", "October 20", "October 23")];
            approve_and_apply(&mut conn, &root, operations)
        };
        conn.execute("UPDATE action_plans SET applied_at = '1' WHERE id = ?1", [&first.id]).unwrap();
        let (second, _) = {
            let operations = vec![edit(&conn, &root, "notes/paalala.md", "Mga paalala", "Mga bagong paalala")];
            approve_and_apply(&mut conn, &root, operations)
        };
        prune_history(&conn, &root.id, 1).unwrap();
        let history = list_history(&conn, &root.id).unwrap();
        assert_eq!(history.iter().map(|entry| (entry.plan_id.as_str(), entry.undoable)).collect::<Vec<_>>(), vec![(second.id.as_str(), true), (first.id.as_str(), false)]);
        assert_eq!(undo_plan(&mut conn, &root, &first.id, T0 + 5, &RealFileSystem).unwrap_err().code, ErrorCode::UndoUnavailable);
    }

    #[test]
    fn operations_deserialize_from_the_frontend_contract() {
        let operation: FileOperation = serde_json::from_str(r#"{"kind":"edit","documentId":"d","expectedContentHash":"h","before":"October 20","after":"October 23"}"#).unwrap();
        assert!(matches!(operation, FileOperation::Edit { .. }));
        let operation: FileOperation = serde_json::from_str(r#"{"kind":"move","documentId":"d","expectedContentHash":"h","destinationRelativePath":"a/b.md"}"#).unwrap();
        assert!(matches!(operation, FileOperation::Move { .. }));
    }
}
