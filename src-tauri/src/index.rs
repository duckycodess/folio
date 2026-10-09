use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use crate::contracts::{OffsetUnit, SourcePassage};
use crate::db::NativeResult;
use crate::error::{error, ErrorCode};
use crate::extract::{self, Extraction, MediaKind};
use crate::identity::{content_hash, document_id, embedding_space_fingerprint, normalize_relative_path, relative_path_below};
use crate::workspace::{self, ScopedRoot};

const MAX_DOCUMENTS: usize = 5000;
const BATCH_SIZE: usize = 50;
const EXCERPT_BEFORE: usize = 120;
const EXCERPT_LENGTH: usize = 360;
const MAX_QUERY_TERMS: usize = 32;
const PASSAGES_PER_RESULT: usize = 3;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_millis() as u64).unwrap_or(0)
}

/// A `DocumentRecord` as known to the persistent index, plus its index status.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexedDocument {
    pub id: String,
    pub workspace_id: String,
    pub relative_path: String,
    pub name: String,
    pub title: String,
    /// Language detection belongs to the provider track; the index does not guess.
    pub language: &'static str,
    pub media_type: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at_ms: Option<u64>,
    /// The revision the index holds. For a `stale` document this is the previous version.
    pub content_hash: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_at_ms: Option<u64>,
}

/// The link of an `explicitReference` relationship: as written, and where it resolved.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinkTarget {
    pub raw_target: String,
    pub resolved_relative_path: String,
}

/// The `explicitReference` member of the frozen `Relationship` union.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExplicitReference {
    pub source_id: String,
    pub target_id: String,
    pub source_content_hash: String,
    pub target_content_hash: String,
    #[serde(rename = "type")]
    pub relationship_type: &'static str,
    pub provenance: &'static str,
    pub link: LinkTarget,
    pub evidence: Vec<SourcePassage>,
}

#[derive(Serialize, Deserialize)]
struct StoredReference {
    link: LinkTarget,
    evidence: Vec<SourcePassage>,
}

/// The frozen `SearchResult`. Keyword results carry no space fingerprint.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub document: IndexedDocument,
    pub passages: Vec<SourcePassage>,
    pub score: f64,
    pub method: &'static str,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub workspace_id: String,
    pub phase: &'static str,
    pub processed: usize,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_path: Option<String>,
}

/// Counts describe what this scan did; `unchanged` documents were not re-extracted.
#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummary {
    pub workspace_id: String,
    pub total: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub removed: usize,
    pub unsupported: usize,
    pub failed: usize,
    pub stale: usize,
    /// Entries that could not be read or identified (e.g. a name that is not valid Unicode).
    pub skipped: usize,
    pub cancelled: bool,
    pub duration_ms: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub content_hash: String,
    pub size_bytes: u64,
    /// Every listed document was re-read and compared byte for byte.
    pub documents: Vec<IndexedDocument>,
}

fn sha256_file(path: &Path) -> NativeResult<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 { break; }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

pub fn passage(document_id: &str, document_hash: &str, start: usize, end: usize, text: &str, page: Option<u32>) -> SourcePassage {
    SourcePassage { document_id: document_id.to_owned(), document_content_hash: document_hash.to_owned(), offset_unit: OffsetUnit::Utf8Byte, start, end, text: text.to_owned(), page }
}

const DOCUMENT_COLUMNS: &str = "id, workspace_id, relative_path, name, COALESCE(title, name), media_type, size_bytes, modified_at, content_hash, status, status_message, indexed_at";

fn document_from_row(row: &Row<'_>) -> rusqlite::Result<IndexedDocument> {
    let modified: String = row.get(7)?;
    let indexed: Option<String> = row.get(11)?;
    Ok(IndexedDocument {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        relative_path: row.get(2)?,
        name: row.get(3)?,
        title: row.get(4)?,
        language: "unknown",
        media_type: row.get(5)?,
        size_bytes: row.get::<_, i64>(6)? as u64,
        modified_at_ms: modified.parse::<u128>().ok().map(|nanos| (nanos / 1_000_000) as u64),
        content_hash: row.get(8)?,
        status: row.get(9)?,
        status_message: row.get(10)?,
        indexed_at_ms: indexed.and_then(|value| value.parse().ok()),
    })
}

pub fn list_documents(conn: &Connection, workspace_id: &str) -> NativeResult<Vec<IndexedDocument>> {
    let mut statement = conn.prepare(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE workspace_id = ?1 ORDER BY relative_path"))?;
    let rows = statement.query_map([workspace_id], document_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_document(conn: &Connection, workspace_id: &str, document_id: &str) -> NativeResult<IndexedDocument> {
    conn.query_row(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE workspace_id = ?1 AND id = ?2"), [workspace_id, document_id], document_from_row)
        .optional()?
        .ok_or_else(|| error(ErrorCode::DocumentUnavailable, "That document is not in this folder's index.").with_detail("documentId", document_id))
}

// ---------------------------------------------------------------- scanning

struct Found {
    relative: String,
    path: PathBuf,
    kind: MediaKind,
    size: i64,
    modified: String,
}

struct Existing {
    id: String,
    size: i64,
    modified: String,
    hash: String,
    status: String,
}

/// What reading one file produced. Computed without touching the index, so a batch
/// holds the SQLite write lock only while its results are stored.
enum Prepared {
    /// Size and modification time match a current record: not read at all.
    Unchanged,
    /// Read, and byte-identical to what the index holds.
    SameBytes,
    Indexed { hash: String, title: Option<String>, note: Option<String>, chunks: Vec<extract::Chunk> },
    Unsupported { hash: String, reason: String },
    Failure { hash: Option<String>, reason: String },
}

enum Outcome {
    Unchanged,
    Added,
    Updated,
    Unsupported,
    Failed,
    Stale,
}

fn skipped_directory(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') || name == "node_modules"
}

fn modified_nanos(metadata: &std::fs::Metadata) -> String {
    metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|elapsed| elapsed.as_nanos().to_string()).unwrap_or_default()
}

/// What a walk of the folder saw.
struct Discovery {
    found: Vec<Found>,
    skipped: usize,
    /// Relative paths (files or folders) that could not be read this time. Records at or
    /// below them are kept as they are rather than treated as deleted.
    unreadable: Vec<String>,
    /// A failure whose location is unknown: nothing may be treated as deleted.
    unreadable_unknown: bool,
}

fn relative_display(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
    (!relative.is_empty()).then_some(relative)
}

/// Lists the supported files below the root. The document limit is checked here, before
/// anything is written, so a folder over the limit is refused rather than partly indexed.
fn discover(root: &Path) -> NativeResult<Discovery> {
    let mut discovery = Discovery { found: Vec::new(), skipped: 0, unreadable: Vec::new(), unreadable_unknown: false };
    let walker = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|entry| entry.depth() == 0 || !(entry.file_type().is_dir() && skipped_directory(entry.file_name())));
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(failure) => {
                discovery.skipped += 1;
                match failure.path().and_then(|path| relative_display(root, path)) {
                    Some(relative) => discovery.unreadable.push(relative),
                    None => discovery.unreadable_unknown = true,
                }
                continue;
            }
        };
        // Symlinks report their own type here, so links never enter the index.
        if !entry.file_type().is_file() || entry.file_name().to_string_lossy().starts_with('.') { continue; }
        let Some(kind) = MediaKind::from_path(entry.path()) else { continue };
        let Ok(relative) = relative_path_below(root, entry.path()) else {
            discovery.skipped += 1;
            continue;
        };
        let Ok(metadata) = entry.metadata() else {
            discovery.skipped += 1;
            discovery.unreadable.push(relative);
            continue;
        };
        discovery.found.push(Found { relative, path: entry.path().to_path_buf(), kind, size: metadata.len() as i64, modified: modified_nanos(&metadata) });
        if discovery.found.len() > MAX_DOCUMENTS {
            return Err(error(ErrorCode::WorkspaceUnavailable, "This folder has more than 5,000 TXT, Markdown and PDF documents. Choose a smaller folder.").with_detail("reason", "tooManyDocuments").with_detail("limit", MAX_DOCUMENTS.to_string()));
        }
    }
    discovery.found.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(discovery)
}

/// Whether `path` is, or lies inside, one of the `unreadable` locations.
fn covered_by(path: &str, unreadable: &[String]) -> bool {
    unreadable.iter().any(|location| path == location || path.strip_prefix(location.as_str()).is_some_and(|rest| rest.starts_with('/')))
}

/// Records that are really gone: not found, and not inside something that could not be read.
fn removed_paths<'a>(existing: impl Iterator<Item = &'a String>, present: &HashSet<&str>, discovery: &Discovery) -> Vec<String> {
    if discovery.unreadable_unknown { return Vec::new(); }
    existing.filter(|path| !present.contains(path.as_str()) && !covered_by(path, &discovery.unreadable)).cloned().collect()
}

/// Incrementally indexes the authorized folder. Unchanged files are not re-read; changed
/// files have their chunks, vectors, relationships and caches replaced; deleted files are
/// removed, while records inside folders that could not be read are kept. Each batch is
/// read and extracted first and only then written, so the write lock is held briefly.
/// Cancellation keeps all completed batches.
pub fn scan_workspace(conn: &mut Connection, root: &ScopedRoot, cancel: &AtomicBool, progress: &mut dyn FnMut(&IndexProgress)) -> NativeResult<ScanSummary> {
    let started = std::time::Instant::now();
    let root_path = workspace::available_root(&root.path)?;
    let report = |phase, processed, total, current_path| IndexProgress { workspace_id: root.id.clone(), phase, processed, total, current_path };
    progress(&report("discovering", 0, 0, None));
    let discovery = discover(&root_path)?;
    let found = &discovery.found;
    let mut summary = ScanSummary { workspace_id: root.id.clone(), total: found.len(), skipped: discovery.skipped, ..Default::default() };

    let mut existing: HashMap<String, Existing> = HashMap::new();
    {
        let mut statement = conn.prepare("SELECT relative_path, id, size_bytes, modified_at, content_hash, status FROM documents WHERE workspace_id = ?1")?;
        let rows = statement.query_map([&root.id], |row| Ok((row.get::<_, String>(0)?, Existing { id: row.get(1)?, size: row.get(2)?, modified: row.get(3)?, hash: row.get(4)?, status: row.get(5)? })))?;
        for row in rows {
            let (path, record) = row?;
            existing.insert(path, record);
        }
    }

    let present: HashSet<&str> = found.iter().map(|file| file.relative.as_str()).collect();
    let removed = removed_paths(existing.keys(), &present, &discovery);
    if !removed.is_empty() {
        let tx = conn.transaction()?;
        for path in &removed { forget_document(&tx, &existing[path].id)?; }
        tx.commit()?;
        summary.removed = removed.len();
    }

    let mut processed = 0;
    for batch in found.chunks(BATCH_SIZE) {
        let mut prepared = Vec::with_capacity(batch.len());
        for file in batch {
            if cancel.load(Ordering::SeqCst) {
                summary.cancelled = true;
                break;
            }
            prepared.push((file, prepare_file(file, existing.get(&file.relative), false)));
        }
        let tx = conn.transaction()?;
        for (file, result) in prepared {
            match store_prepared(&tx, &root.id, file, existing.get(&file.relative), result)? {
                Outcome::Unchanged => summary.unchanged += 1,
                Outcome::Added => summary.added += 1,
                Outcome::Updated => summary.updated += 1,
                Outcome::Unsupported => summary.unsupported += 1,
                Outcome::Failed => summary.failed += 1,
                Outcome::Stale => summary.stale += 1,
            }
            processed += 1;
        }
        tx.commit()?;
        progress(&report("indexing", processed, found.len(), batch.last().map(|file| file.relative.clone())));
        if summary.cancelled { break; }
    }

    if summary.added + summary.updated + summary.removed + summary.unsupported + summary.failed > 0 {
        progress(&report("linking", processed, found.len(), None));
        let tx = conn.transaction()?;
        rebuild_explicit_references(&tx, &root.id)?;
        tx.commit()?;
    }
    summary.duration_ms = started.elapsed().as_millis() as u64;
    progress(&report(if summary.cancelled { "cancelled" } else { "done" }, processed, found.len(), None));
    Ok(summary)
}

/// Reads, hashes and extracts one file. Never touches the index. A `stale` or `failed`
/// record is always re-read, so a file that was locked, offline or briefly unreadable
/// recovers on the next scan. `verify` also re-reads current records (files Folio wrote).
fn prepare_file(file: &Found, prior: Option<&Existing>, verify: bool) -> Prepared {
    if let Some(prior) = prior {
        let retry = prior.status == "stale" || prior.status == "failed";
        if !verify && !retry && prior.size == file.size && prior.modified == file.modified { return Prepared::Unchanged; }
    }
    let bytes = match workspace::read_bounded(&file.path, file.kind.max_bytes()) {
        Ok(bytes) => Some(bytes),
        Err(failure) if failure.code == ErrorCode::DocumentTooLarge => None,
        Err(failure) => return Prepared::Failure { hash: None, reason: failure.message },
    };
    let hash = match &bytes {
        Some(bytes) => content_hash(bytes),
        None => match sha256_file(&file.path) {
            Ok(hash) => hash,
            Err(failure) => return Prepared::Failure { hash: None, reason: failure.message },
        },
    };
    // Identical bytes need no extraction, except for a `failed` record, which is retried
    // in case the failure came from the extractor rather than the file.
    if prior.is_some_and(|prior| prior.hash == hash && prior.status != "failed") { return Prepared::SameBytes; }
    let Some(bytes) = bytes else {
        return Prepared::Unsupported { hash, reason: format!("Larger than the {} MiB limit for this file type.", file.kind.max_bytes() / 1024 / 1024) };
    };
    match extract::extract(file.kind, &bytes) {
        Ok(Extraction::Text(extracted)) => {
            let note = (!extracted.skipped_pages.is_empty()).then(|| {
                let pages: Vec<String> = extracted.skipped_pages.iter().map(u32::to_string).collect();
                format!("Page(s) {} could not be read; the rest of the document is searchable.", pages.join(", "))
            });
            Prepared::Indexed { title: extract::title_of(&extracted.text), chunks: extract::chunk(&extracted), hash, note }
        }
        Ok(Extraction::Unsupported(reason)) => Prepared::Unsupported { hash, reason },
        Err(failure) => Prepared::Failure { hash: Some(hash), reason: failure.message },
    }
}

/// Writes one prepared result. Runs inside the batch transaction and does no file I/O.
fn store_prepared(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>, prepared: Prepared) -> NativeResult<Outcome> {
    match prepared {
        Prepared::Unchanged => Ok(Outcome::Unchanged),
        Prepared::SameBytes => {
            // Refresh metadata only. A stale record whose file is back to its indexed
            // content is current again.
            let prior = prior.expect("identical bytes imply a prior record");
            tx.execute(
                "UPDATE documents SET size_bytes = ?1, modified_at = ?2, status = CASE status WHEN 'stale' THEN 'indexed' ELSE status END, status_message = CASE status WHEN 'stale' THEN NULL ELSE status_message END WHERE id = ?3",
                params![file.size, file.modified, prior.id],
            )?;
            Ok(Outcome::Unchanged)
        }
        Prepared::Indexed { hash, title, note, chunks } => {
            let id = upsert_document(tx, workspace_id, file, prior, &hash, title.as_deref(), "indexed", note.as_deref(), true)?;
            clear_derived(tx, &id)?;
            let mut insert = tx.prepare_cached("INSERT INTO chunks (document_id, ordinal, chunk_text, start_offset, end_offset, page, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
            for chunk in chunks {
                insert.execute(params![id, chunk.ordinal as i64, chunk.text, chunk.start as i64, chunk.end as i64, chunk.page, content_hash(chunk.text.as_bytes())])?;
            }
            Ok(if prior.is_some() { Outcome::Updated } else { Outcome::Added })
        }
        Prepared::Unsupported { hash, reason } => {
            let id = upsert_document(tx, workspace_id, file, prior, &hash, None, "unsupported", Some(&reason), false)?;
            clear_derived(tx, &id)?;
            Ok(Outcome::Unsupported)
        }
        Prepared::Failure { hash, reason } => record_failure(tx, workspace_id, file, prior, &reason, hash.as_deref()),
    }
}

#[allow(clippy::too_many_arguments)]
fn upsert_document(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>, hash: &str, title: Option<&str>, status: &str, message: Option<&str>, indexed: bool) -> NativeResult<String> {
    let name = file.relative.rsplit('/').next().unwrap_or(&file.relative).to_owned();
    let indexed_at = indexed.then(|| now_ms().to_string());
    match prior {
        Some(prior) => {
            tx.execute(
                "UPDATE documents SET name = ?1, title = ?2, content_hash = ?3, media_type = ?4, size_bytes = ?5, modified_at = ?6, indexed_at = COALESCE(?7, indexed_at), status = ?8, status_message = ?9 WHERE id = ?10",
                params![name, title, hash, file.kind.media_type(), file.size, file.modified, indexed_at, status, message, prior.id],
            )?;
            Ok(prior.id.clone())
        }
        None => {
            let id = document_id(workspace_id, &file.relative);
            tx.execute(
                "INSERT INTO documents (id, workspace_id, relative_path, name, title, content_hash, media_type, size_bytes, modified_at, indexed_at, status, status_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![id, workspace_id, file.relative, name, title, hash, file.kind.media_type(), file.size, file.modified, indexed_at, status, message],
            )?;
            Ok(id)
        }
    }
}

/// Extraction failure never erases valid prior state: an indexed document keeps its chunks
/// and becomes `stale`; a document with no usable prior index is recorded as `failed`.
/// Both are retried on every scan until a read succeeds.
fn record_failure(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>, reason: &str, hash: Option<&str>) -> NativeResult<Outcome> {
    match prior {
        Some(prior) if prior.status == "indexed" || prior.status == "stale" => {
            tx.execute(
                "UPDATE documents SET size_bytes = ?1, modified_at = ?2, status = 'stale', status_message = ?3 WHERE id = ?4",
                params![file.size, file.modified, format!("The file changed but could not be re-read; search shows the previous version. {reason}"), prior.id],
            )?;
            Ok(Outcome::Stale)
        }
        _ => {
            let id = upsert_document(tx, workspace_id, file, prior, hash.unwrap_or(""), None, "failed", Some(reason), false)?;
            clear_derived(tx, &id)?;
            Ok(Outcome::Failed)
        }
    }
}

/// Drops everything derived from a document's previous content. Embeddings cascade from chunks.
pub fn clear_derived(tx: &Transaction<'_>, document_id: &str) -> NativeResult<()> {
    tx.execute("DELETE FROM chunks WHERE document_id = ?1", [document_id])?;
    tx.execute("DELETE FROM relationships WHERE source_document_id = ?1 OR target_document_id = ?1", [document_id])?;
    tx.execute("DELETE FROM derived_cache WHERE document_id = ?1", [document_id])?;
    Ok(())
}

pub fn forget_document(tx: &Transaction<'_>, document_id: &str) -> NativeResult<()> {
    // History survives the document so applied changes stay recoverable.
    tx.execute("UPDATE history SET document_id = NULL WHERE document_id = ?1", [document_id])?;
    tx.execute("DELETE FROM documents WHERE id = ?1", [document_id])?;
    Ok(())
}

fn existing_at(conn: &Connection, workspace_id: &str, relative: &str) -> NativeResult<Option<Existing>> {
    Ok(conn
        .query_row("SELECT id, size_bytes, modified_at, content_hash, status FROM documents WHERE workspace_id = ?1 AND relative_path = ?2", [workspace_id, relative], |row| {
            Ok(Existing { id: row.get(0)?, size: row.get(1)?, modified: row.get(2)?, hash: row.get(3)?, status: row.get(4)? })
        })
        .optional()?)
}

/// Re-indexes specific files after Folio changed them, then rebuilds link relationships.
/// Paths that no longer exist are removed from the index. Files are read before the
/// write transaction opens.
#[allow(dead_code)]
pub fn refresh_paths(conn: &mut Connection, root: &ScopedRoot, relative_paths: &[String]) -> NativeResult<()> {
    let mut prepared = Vec::new();
    let mut gone = Vec::new();
    for relative in relative_paths {
        let prior = existing_at(conn, &root.id, relative)?;
        let path = match workspace::resolve_document(&root.path, relative) {
            Ok(path) => path,
            Err(failure) if failure.code == ErrorCode::DocumentUnavailable => {
                if let Some(prior) = prior { gone.push(prior.id); }
                continue;
            }
            Err(failure) => return Err(failure),
        };
        let kind = MediaKind::from_path(&path).ok_or_else(|| error(ErrorCode::UnsupportedMediaType, "Folio indexes TXT, Markdown and text-based PDF files."))?;
        let metadata = std::fs::metadata(&path)?;
        let file = Found { relative: relative.clone(), path, kind, size: metadata.len() as i64, modified: modified_nanos(&metadata) };
        let result = prepare_file(&file, prior.as_ref(), true);
        prepared.push((file, prior, result));
    }
    let tx = conn.transaction()?;
    for id in &gone { forget_document(&tx, id)?; }
    for (file, prior, result) in prepared {
        store_prepared(&tx, &root.id, &file, prior.as_ref(), result)?;
    }
    rebuild_explicit_references(&tx, &root.id)?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------- explicit references

fn has_scheme_or_root(value: &str) -> bool {
    if value.starts_with(['/', '\\']) { return true; }
    let letters = value.chars().take_while(|ch| ch.is_ascii_alphabetic()).count();
    letters > 0 && value[letters..].starts_with(':')
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = value.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// Mirrors the frontend's link resolution: relative links only, never above the root,
/// and the result must be a valid workspace path. Matching is case-sensitive on purpose,
/// like the frontend and like document identities: on a case-insensitive Windows or macOS
/// folder, `[x](Notes.md)` does not resolve to `notes.md`, so no link is invented.
pub fn linked_path(source_path: &str, link: &str) -> Option<String> {
    if has_scheme_or_root(link) { return None; }
    let decoded = percent_decode(link.split(['?', '#']).next().unwrap_or(""))?;
    if decoded.is_empty() || has_scheme_or_root(&decoded) { return None; }
    let mut parts: Vec<&str> = source_path.split('/').collect();
    parts.pop();
    for part in decoded.split('/') {
        match part {
            "" | "." => {}
            ".." => { parts.pop()?; }
            other => parts.push(other),
        }
    }
    normalize_relative_path(&parts.join("/")).ok()
}

/// Rebuilds Markdown-link relationships for the workspace, one per distinct link, with the
/// located link text as evidence.
pub fn rebuild_explicit_references(tx: &Transaction<'_>, workspace_id: &str) -> NativeResult<usize> {
    tx.execute(
        "DELETE FROM relationships WHERE relationship_type = 'explicitReference' AND provenance = 'documentLink' AND source_document_id IN (SELECT id FROM documents WHERE workspace_id = ?1)",
        [workspace_id],
    )?;
    let mut by_path: HashMap<String, (String, String)> = HashMap::new();
    {
        let mut statement = tx.prepare("SELECT relative_path, id, content_hash FROM documents WHERE workspace_id = ?1 AND status IN ('indexed','stale','unsupported')")?;
        for row in statement.query_map([workspace_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))? {
            let (path, id, hash) = row?;
            by_path.insert(path, (id, hash));
        }
    }
    let mut edges: Vec<((String, String, String), StoredReference)> = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT d.id, d.relative_path, d.content_hash, c.chunk_text, c.start_offset, c.page FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.workspace_id = ?1 AND c.chunk_text LIKE '%](%' ORDER BY d.relative_path, c.ordinal",
        )?;
        let rows = statement.query_map([workspace_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, Option<u32>>(5)?)))?;
        for row in rows {
            let (source_id, source_path, source_hash, text, chunk_start, page) = row?;
            for link in extract::markdown_links(&text) {
                let Some(resolved) = linked_path(&source_path, &link.target) else { continue };
                let Some((target_id, _)) = by_path.get(&resolved) else { continue };
                if *target_id == source_id { continue; }
                let evidence = passage(&source_id, &source_hash, chunk_start as usize + link.whole.start, chunk_start as usize + link.whole.end, &text[link.whole.clone()], page);
                let key = (source_id.clone(), target_id.clone(), link.target.clone());
                match edges.iter_mut().find(|(existing, _)| *existing == key) {
                    Some((_, stored)) => stored.evidence.push(evidence),
                    None => edges.push((key, StoredReference { link: LinkTarget { raw_target: link.target.clone(), resolved_relative_path: resolved }, evidence: vec![evidence] })),
                }
            }
        }
    }
    let hash_of: HashMap<&String, &String> = by_path.values().map(|(id, hash)| (id, hash)).collect();
    let now = now_ms().to_string();
    for ((source, target, raw), stored) in &edges {
        let id = content_hash(format!("explicitReference\0{source}\0{target}\0{raw}").as_bytes());
        tx.execute(
            "INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES (?1, ?2, ?3, 'explicitReference', ?4, 'documentLink', NULL, ?5, ?6, ?7)",
            params![id, source, target, serde_json::to_string(stored)?, hash_of[source], hash_of[target], now],
        )?;
    }
    Ok(edges.len())
}

pub fn list_relationships(conn: &Connection, workspace_id: &str) -> NativeResult<Vec<ExplicitReference>> {
    let mut statement = conn.prepare(
        "SELECT r.source_document_id, r.target_document_id, r.evidence_json, r.source_content_hash, r.target_content_hash FROM relationships r JOIN documents d ON d.id = r.source_document_id WHERE d.workspace_id = ?1 AND r.relationship_type = 'explicitReference' ORDER BY d.relative_path, r.target_document_id",
    )?;
    let rows = statement.query_map([workspace_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)))?;
    let mut relationships = Vec::new();
    for row in rows {
        let (source_id, target_id, json, source_content_hash, target_content_hash) = row?;
        let stored: StoredReference = serde_json::from_str(&json)?;
        relationships.push(ExplicitReference { source_id, target_id, source_content_hash, target_content_hash, relationship_type: "explicitReference", provenance: "documentLink", link: stored.link, evidence: stored.evidence });
    }
    Ok(relationships)
}

// ---------------------------------------------------------------- keyword search

fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for term in query.split(|ch: char| !ch.is_alphanumeric()).filter(|term| !term.is_empty()) {
        let term = term.to_lowercase();
        if !terms.contains(&term) { terms.push(term); }
    }
    terms.truncate(MAX_QUERY_TERMS);
    terms
}

fn find_word(haystack: &str, term: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(term) {
        let start = from + offset;
        let end = start + term.len();
        let before = haystack[..start].chars().next_back().is_none_or(|ch| !ch.is_alphanumeric());
        let after = haystack[end..].chars().next().is_none_or(|ch| !ch.is_alphanumeric());
        if before && after { return Some(start); }
        from = start + haystack[start..].chars().next().map_or(1, char::len_utf8);
    }
    None
}

/// Where FTS5 placed its first match marker. `highlight()` inserts the markers into the
/// original chunk text, so the offset of the first marker is the match's offset there —
/// including matches FTS5 made by folding case or accents ("cafe" for "café").
fn highlighted_offset(highlighted: &str, chunk_text: &str) -> Option<usize> {
    highlighted.find(MATCH_START).filter(|offset| chunk_text.is_char_boundary(*offset))
}

const MATCH_START: char = '\u{1}';

/// A window of the chunk around the first match, located with UTF-8 byte offsets.
fn excerpt(document_id: &str, document_hash: &str, chunk_text: &str, highlighted: &str, chunk_start: usize, page: Option<u32>, terms: &[String]) -> SourcePassage {
    let lower = chunk_text.to_lowercase();
    let hit = highlighted_offset(highlighted, chunk_text)
        .or_else(|| (lower.len() == chunk_text.len()).then(|| terms.iter().filter_map(|term| find_word(&lower, term)).min()).flatten())
        .unwrap_or(0);
    let boundaries: Vec<usize> = chunk_text.char_indices().map(|(index, _)| index).chain(std::iter::once(chunk_text.len())).collect();
    let hit_char = boundaries.partition_point(|&index| index < hit);
    let from_char = hit_char.saturating_sub(EXCERPT_BEFORE);
    let to_char = (from_char + EXCERPT_LENGTH).min(boundaries.len() - 1);
    let (from, to) = (boundaries[from_char], boundaries[to_char]);
    passage(document_id, document_hash, chunk_start + from, chunk_start + to, &chunk_text[from..to], page)
}

/// FTS5 keyword search over indexed chunks. Query words are quoted and OR-ed, so document
/// or query text cannot inject FTS syntax. This is keyword matching, not semantic search.
pub fn search(conn: &Connection, workspace_id: &str, query: &str, limit: usize) -> NativeResult<Vec<SearchResult>> {
    let terms = query_terms(query);
    if terms.is_empty() { return Ok(Vec::new()); }
    let expression = terms.iter().map(|term| format!("\"{term}\"")).collect::<Vec<_>>().join(" OR ");
    let limit = limit.clamp(1, 100);
    let mut statement = conn.prepare(
        "SELECT c.document_id, d.content_hash, c.chunk_text, c.start_offset, c.page, bm25(chunks_fts) AS rank, highlight(chunks_fts, 0, char(1), char(2)) FROM chunks_fts JOIN chunks c ON c.chunk_id = chunks_fts.rowid JOIN documents d ON d.id = c.document_id WHERE chunks_fts MATCH ?1 AND d.workspace_id = ?2 ORDER BY rank LIMIT ?3",
    )?;
    let rows = statement.query_map(params![expression, workspace_id, (limit * 8).min(400) as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<u32>>(4)?, row.get::<_, f64>(5)?, row.get::<_, String>(6)?))
    })?;
    let mut grouped: Vec<(String, f64, Vec<SourcePassage>)> = Vec::new();
    for row in rows {
        let (document_id, hash, text, start, page, rank, highlighted) = row?;
        let found = excerpt(&document_id, &hash, &text, &highlighted, start as usize, page, &terms);
        match grouped.iter().position(|(id, _, _)| *id == document_id) {
            Some(index) if grouped[index].2.len() < PASSAGES_PER_RESULT => grouped[index].2.push(found),
            Some(_) => {}
            None if grouped.len() < limit => grouped.push((document_id, -rank, vec![found])),
            None => {}
        }
    }
    grouped
        .into_iter()
        .map(|(document_id, score, passages)| Ok(SearchResult { document: get_document(conn, workspace_id, &document_id)?, passages, score, method: "keyword" }))
        .collect()
}

// ---------------------------------------------------------------- exact duplicates

/// Documents that share a content hash: candidates only, read from the index. Cheap, so it
/// can run while the index is locked; `verify_duplicates` does the file I/O afterwards.
pub fn duplicate_candidates(conn: &Connection, workspace_id: &str) -> NativeResult<Vec<(String, Vec<IndexedDocument>)>> {
    let mut statement = conn.prepare(
        "SELECT content_hash FROM documents WHERE workspace_id = ?1 AND content_hash != '' AND status IN ('indexed','unsupported') GROUP BY content_hash HAVING count(*) > 1 ORDER BY content_hash",
    )?;
    let hashes: Vec<String> = statement.query_map([workspace_id], |row| row.get(0))?.collect::<Result<_, _>>()?;
    let mut candidates = Vec::new();
    for hash in hashes {
        let mut members = conn.prepare(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE workspace_id = ?1 AND content_hash = ?2 ORDER BY relative_path"))?;
        let documents = members.query_map([workspace_id, &hash], document_from_row)?.collect::<Result<_, _>>()?;
        candidates.push((hash, documents));
    }
    Ok(candidates)
}

fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            read => filled += read,
        }
    }
    Ok(filled)
}

/// Compares two files byte for byte in fixed-size blocks, so files of any size are verified
/// without loading them into memory.
fn same_bytes(a: &Path, b: &Path) -> std::io::Result<bool> {
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() { return Ok(false); }
    let (mut left, mut right) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
    let (mut left_block, mut right_block) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let (read_left, read_right) = (fill(&mut left, &mut left_block)?, fill(&mut right, &mut right_block)?);
        if read_left != read_right || left_block[..read_left] != right_block[..read_right] { return Ok(false); }
        if read_left == 0 { return Ok(true); }
    }
}

/// Confirms candidate groups by re-reading the files: the stored hash only nominates them.
/// Files that cannot be read now are left out of the group rather than assumed identical.
pub fn verify_duplicates(root: &Path, candidates: Vec<(String, Vec<IndexedDocument>)>) -> Vec<DuplicateGroup> {
    let mut groups = Vec::new();
    for (hash, documents) in candidates {
        let mut confirmed: Vec<(PathBuf, Vec<IndexedDocument>)> = Vec::new();
        for document in documents {
            let Ok(path) = workspace::resolve_document(root, &document.relative_path) else { continue };
            match confirmed.iter_mut().find(|(representative, _)| same_bytes(representative, &path).unwrap_or(false)) {
                Some((_, members)) => members.push(document),
                None => confirmed.push((path, vec![document])),
            }
        }
        for (representative, documents) in confirmed {
            if documents.len() > 1 && sha256_file(&representative).ok().as_deref() == Some(hash.as_str()) {
                groups.push(DuplicateGroup { content_hash: hash.clone(), size_bytes: documents[0].size_bytes, documents });
            }
        }
    }
    groups
}

/// Groups documents whose bytes are identical.
#[cfg(test)]
pub fn duplicate_groups(conn: &Connection, root: &ScopedRoot) -> NativeResult<Vec<DuplicateGroup>> {
    Ok(verify_duplicates(&root.path, duplicate_candidates(conn, &root.id)?))
}

// ---------------------------------------------------------------- embedding store

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSpace {
    pub model_id: String,
    pub revision: String,
    pub quantization: String,
    pub dimensions: u32,
    pub preprocessing_fingerprint: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChunkVector {
    pub chunk_id: i64,
    /// The `contentHash` of the chunk text the vector was computed from, as listed by
    /// `pending_embedding_chunks`. Chunk ids can be reused after a rescan, so a vector is
    /// only stored while the chunk still holds that text.
    pub content_hash: String,
    pub vector: Vec<f32>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingChunk {
    pub chunk_id: i64,
    pub document_id: String,
    pub text: String,
    /// Echo this back in `ChunkVector` so a vector for replaced text is refused.
    pub content_hash: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VectorCandidate {
    pub chunk_id: i64,
    pub score: f32,
    pub space_fingerprint: String,
    pub passage: SourcePassage,
}

/// Stores the space under its frozen fingerprint (`folio-space-v1/...`) and returns it.
pub fn register_space(conn: &Connection, space: &EmbeddingSpace) -> NativeResult<String> {
    let fields = [&space.model_id, &space.revision, &space.quantization, &space.preprocessing_fingerprint];
    if fields.iter().any(|field| field.trim().is_empty()) || space.dimensions == 0 || space.dimensions > 8192 {
        return Err(error(ErrorCode::EmbeddingSpaceMismatch, "An embedding space needs a model, revision, quantization, preprocessing fingerprint and 1–8192 dimensions."));
    }
    let fingerprint = embedding_space_fingerprint(&space.model_id, &space.revision, &space.quantization, space.dimensions, &space.preprocessing_fingerprint);
    conn.execute(
        "INSERT OR IGNORE INTO embedding_spaces (id, model_id, revision, quantization, dimensions, preprocessing_fingerprint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![fingerprint, space.model_id, space.revision, space.quantization, space.dimensions, space.preprocessing_fingerprint],
    )?;
    Ok(fingerprint)
}

fn space_dimensions(conn: &Connection, fingerprint: &str) -> NativeResult<usize> {
    conn.query_row("SELECT dimensions FROM embedding_spaces WHERE id = ?1", [fingerprint], |row| row.get::<_, i64>(0))
        .optional()?
        .map(|dimensions| dimensions as usize)
        .ok_or_else(|| error(ErrorCode::EmbeddingSpaceMismatch, "Unknown embedding space. Register it before storing or comparing vectors.").with_detail("spaceFingerprint", fingerprint))
}

fn check_vector(vector: &[f32], dimensions: usize) -> NativeResult<()> {
    if vector.len() != dimensions {
        return Err(error(ErrorCode::EmbeddingSpaceMismatch, format!("Expected a {dimensions}-dimension vector for this embedding space.")).with_detail("receivedDimensions", vector.len().to_string()));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(error(ErrorCode::EmbeddingSpaceMismatch, "Vectors must contain finite numbers."));
    }
    Ok(())
}

pub fn put_embeddings(conn: &mut Connection, workspace_id: &str, fingerprint: &str, items: &[ChunkVector]) -> NativeResult<usize> {
    let dimensions = space_dimensions(conn, fingerprint)?;
    let tx = conn.transaction()?;
    for item in items {
        check_vector(&item.vector, dimensions)?;
        let current: Option<String> = tx
            .query_row("SELECT c.content_hash FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.chunk_id = ?1 AND d.workspace_id = ?2", params![item.chunk_id, workspace_id], |row| row.get(0))
            .optional()?;
        let refuse = |reason: &str| error(ErrorCode::EvidenceInvalid, "That chunk changed or left this folder's index. Fetch the pending chunks again.").with_detail("chunkId", item.chunk_id.to_string()).with_detail("reason", reason);
        match current {
            None => return Err(refuse("chunkMissing")),
            Some(hash) if hash != item.content_hash => return Err(refuse("chunkChanged")),
            Some(_) => {}
        }
        let blob: Vec<u8> = item.vector.iter().flat_map(|value| value.to_le_bytes()).collect();
        tx.execute("INSERT OR REPLACE INTO embeddings (chunk_id, space_id, vector) VALUES (?1, ?2, ?3)", params![item.chunk_id, fingerprint, blob])?;
    }
    tx.commit()?;
    Ok(items.len())
}

/// Chunks that have no vector in this space yet, for the embedding provider to process.
pub fn pending_embedding_chunks(conn: &Connection, workspace_id: &str, fingerprint: &str, limit: usize) -> NativeResult<Vec<PendingChunk>> {
    space_dimensions(conn, fingerprint)?;
    let mut statement = conn.prepare(
        "SELECT c.chunk_id, c.document_id, c.chunk_text, c.content_hash FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.workspace_id = ?1 AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.chunk_id = c.chunk_id AND e.space_id = ?2) ORDER BY c.chunk_id LIMIT ?3",
    )?;
    let rows = statement.query_map(params![workspace_id, fingerprint, limit.clamp(1, 512) as i64], |row| Ok(PendingChunk { chunk_id: row.get(0)?, document_id: row.get(1)?, text: row.get(2)?, content_hash: row.get(3)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Exact cosine search within one embedding space. Vectors from other spaces are never read.
pub fn vector_candidates(conn: &Connection, workspace_id: &str, fingerprint: &str, query: &[f32], k: usize) -> NativeResult<Vec<VectorCandidate>> {
    check_vector(query, space_dimensions(conn, fingerprint)?)?;
    let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
    let mut statement = conn.prepare(
        "SELECT e.chunk_id, c.document_id, d.content_hash, c.chunk_text, c.start_offset, c.end_offset, c.page, e.vector FROM embeddings e JOIN chunks c ON c.chunk_id = e.chunk_id JOIN documents d ON d.id = c.document_id WHERE e.space_id = ?1 AND d.workspace_id = ?2",
    )?;
    let rows = statement.query_map(params![fingerprint, workspace_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?, row.get::<_, Option<u32>>(6)?, row.get::<_, Vec<u8>>(7)?))
    })?;
    let mut candidates = Vec::new();
    for row in rows {
        let (chunk_id, document_id, hash, text, start, end, page, blob) = row?;
        let vector: Vec<f32> = blob.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
        if vector.len() != query.len() { continue; }
        let dot: f32 = vector.iter().zip(query).map(|(a, b)| a * b).sum();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        let score = if norm == 0.0 || query_norm == 0.0 { 0.0 } else { dot / (norm * query_norm) };
        candidates.push(VectorCandidate { chunk_id, score, space_fingerprint: fingerprint.to_owned(), passage: passage(&document_id, &hash, start as usize, end as usize, &text, page) });
    }
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.truncate(k.clamp(1, 100));
    Ok(candidates)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{db, extract::testpdf, workspace::WorkspaceRegistry};
    use std::fs;

    pub fn copy_fixtures(destination: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/documents");
        for entry in WalkDir::new(&source) {
            let entry = entry.unwrap();
            let target = destination.join(entry.path().strip_prefix(&source).unwrap());
            if entry.file_type().is_dir() { fs::create_dir_all(&target).unwrap(); } else { fs::copy(entry.path(), &target).unwrap(); }
        }
    }

    pub fn authorize(conn: &Connection, folder: &Path) -> ScopedRoot {
        let info = WorkspaceRegistry::new().authorize(folder).unwrap();
        workspace::remember(conn, &info).unwrap();
        ScopedRoot { id: info.id, path: PathBuf::from(info.root_path) }
    }

    pub fn scan(conn: &mut Connection, root: &ScopedRoot) -> ScanSummary {
        scan_workspace(conn, root, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    pub fn fixture_workspace() -> (tempfile::TempDir, Connection, ScopedRoot) {
        let folder = tempfile::tempdir().unwrap();
        copy_fixtures(folder.path());
        let conn = db::open_in_memory().unwrap();
        let root = authorize(&conn, folder.path());
        (folder, conn, root)
    }

    pub fn id_of(root: &ScopedRoot, path: &str) -> String {
        document_id(&root.id, path)
    }

    fn status_of(conn: &Connection, root: &ScopedRoot, path: &str) -> (String, Option<String>) {
        conn.query_row("SELECT status, status_message FROM documents WHERE workspace_id = ?1 AND relative_path = ?2", [&root.id, path], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }

    fn chunk_count(conn: &Connection, document_id: &str) -> i64 {
        conn.query_row("SELECT count(*) FROM chunks WHERE document_id = ?1", [document_id], |row| row.get(0)).unwrap()
    }

    fn paths(hits: &[SearchResult]) -> Vec<&str> {
        hits.iter().map(|hit| hit.document.relative_path.as_str()).collect()
    }

    pub fn assert_located(folder: &Path, relative: &str, found: &SourcePassage) {
        let bytes = fs::read(folder.join(relative)).unwrap();
        assert_eq!(found.offset_unit, OffsetUnit::Utf8Byte);
        assert_eq!(&bytes[found.start..found.end], found.text.as_bytes());
        assert_eq!(found.document_content_hash, content_hash(&bytes));
    }

    #[test]
    fn scan_indexes_text_markdown_and_text_pdf_and_marks_scanned_pdf_unsupported() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::write(folder.path().join("research/consent-guide.pdf"), testpdf::text_pdf(&[&["Interview consent guide."], &["Pirmahan ang consent form bago ang panayam."]])).unwrap();
        fs::write(folder.path().join("research/scanned.pdf"), testpdf::image_only_pdf()).unwrap();
        fs::write(folder.path().join("notes/plain.txt"), "Plain text note about the xylophone.").unwrap();
        let summary = scan(&mut conn, &root);
        assert_eq!(summary.unsupported, 1);
        assert_eq!(summary.failed + summary.stale, 0);
        assert_eq!(summary.added, summary.total - 1);
        let (status, message) = status_of(&conn, &root, "research/scanned.pdf");
        assert_eq!(status, "unsupported");
        assert!(message.unwrap().contains("OCR"));
        let pdf_hits = search(&conn, &root.id, "pirmahan", 10).unwrap();
        assert_eq!(paths(&pdf_hits), vec!["research/consent-guide.pdf"]);
        assert_eq!(pdf_hits[0].passages[0].page, Some(2));
        assert_eq!(paths(&search(&conn, &root.id, "xylophone", 10).unwrap()), vec!["notes/plain.txt"]);
    }

    #[test]
    fn search_finds_unopened_documents_with_identity_path_and_located_excerpt() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let hits = search(&conn, &root.id, "huling araw ng pagpasa", 10).unwrap();
        let hit = hits.iter().find(|hit| hit.document.relative_path == "notes/tala-sa-proyekto.md").expect("Filipino note is found");
        assert_eq!(hit.method, "keyword");
        assert_eq!(hit.document.id, format!("{}:notes/tala-sa-proyekto.md", root.id));
        assert!(hit.passages[0].text.contains("huling araw ng pagpasa"));
        assert_located(folder.path(), "notes/tala-sa-proyekto.md", &hit.passages[0]);
    }

    #[test]
    fn excerpt_offsets_are_utf8_bytes_on_non_ascii_text() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let hits = search(&conn, &root.id, "authoritative schedule", 5).unwrap();
        assert_eq!(hits[0].document.relative_path, "projects/submission-checklist.md");
        assert_located(folder.path(), "projects/submission-checklist.md", &hits[0].passages[0]);
    }

    #[test]
    fn an_accent_folded_match_is_located_in_the_excerpt() {
        let (folder, mut conn, root) = fixture_workspace();
        let filler = "Walang kinalaman na pangungusap. ".repeat(20);
        fs::write(folder.path().join("notes/kape.md"), format!("# Kape\n\n{filler}Nagkita kami sa café sa Quezon City.\n")).unwrap();
        scan(&mut conn, &root);
        let hits = search(&conn, &root.id, "cafe", 5).unwrap();
        assert_eq!(hits[0].document.relative_path, "notes/kape.md");
        assert!(hits[0].passages[0].text.contains("café"), "the excerpt shows why it matched");
        assert_located(folder.path(), "notes/kape.md", &hits[0].passages[0]);
    }

    #[test]
    fn large_duplicates_are_verified_by_streaming() {
        let (folder, mut conn, root) = fixture_workspace();
        let big: Vec<u8> = (0..(extract::MAX_PDF_BYTES as usize + 1024)).map(|index| (index % 251) as u8).collect();
        fs::write(folder.path().join("research/export-a.pdf"), &big).unwrap();
        fs::write(folder.path().join("research/export-b.pdf"), &big).unwrap();
        scan(&mut conn, &root);
        let groups = duplicate_groups(&conn, &root).unwrap();
        assert!(groups.iter().any(|group| group.documents.iter().map(|document| document.relative_path.as_str()).collect::<Vec<_>>() == vec!["research/export-a.pdf", "research/export-b.pdf"]));
    }

    #[test]
    fn query_syntax_cannot_break_search() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        for query in ["\"unbalanced", "deadline OR", "NEAR(a b)", "*", "col:umn", ""] {
            assert!(search(&conn, &root.id, query, 5).is_ok(), "query {query:?} failed");
        }
    }

    #[test]
    fn unchanged_rescan_reprocesses_nothing() {
        let (_folder, mut conn, root) = fixture_workspace();
        let first = scan(&mut conn, &root);
        let second = scan(&mut conn, &root);
        assert_eq!(second.unchanged, first.total);
        assert_eq!(second.added + second.updated + second.removed + second.unsupported + second.failed + second.stale, 0);
    }

    #[test]
    fn external_edit_and_delete_update_the_right_records() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let plan_id = id_of(&root, "projects/project-plan.md");
        let notes_id = id_of(&root, "meetings/meeting-notes.md");
        let relationship_count = |conn: &Connection, id: &str| -> i64 { conn.query_row("SELECT count(*) FROM relationships WHERE source_document_id = ?1", [id], |row| row.get(0)).unwrap() };
        assert_eq!(relationship_count(&conn, &plan_id), 2);
        conn.execute("INSERT INTO derived_cache (document_id, kind, content_hash, payload, created_at) VALUES (?1, 'summary', 'x', 'old summary', '0')", [&plan_id]).unwrap();

        fs::write(folder.path().join("projects/project-plan.md"), "# Community Learning Project\n\nThe deadline moved to November 3. No links remain.\n").unwrap();
        fs::remove_file(folder.path().join("personal/grocery-list.md")).unwrap();
        let summary = scan(&mut conn, &root);
        assert_eq!((summary.updated, summary.removed), (1, 1));
        assert_eq!(paths(&search(&conn, &root.id, "November", 5).unwrap()), vec!["projects/project-plan.md"]);
        assert!(!paths(&search(&conn, &root.id, "volunteer", 10).unwrap()).contains(&"projects/project-plan.md"));
        assert_eq!(relationship_count(&conn, &plan_id), 0, "links removed from the edited file are dropped");
        assert_eq!(relationship_count(&conn, &notes_id), 1, "links into the edited file are rebuilt");
        let into_plan = list_relationships(&conn, &root.id).unwrap().into_iter().find(|edge| edge.source_id == notes_id).unwrap();
        assert_eq!(into_plan.target_content_hash, content_hash(&fs::read(folder.path().join("projects/project-plan.md")).unwrap()), "evidence names the new revision");
        let cached: i64 = conn.query_row("SELECT count(*) FROM derived_cache WHERE document_id = ?1", [&plan_id], |row| row.get(0)).unwrap();
        assert_eq!(cached, 0, "cached summary invalidated");
        assert!(search(&conn, &root.id, "grocery", 5).unwrap().is_empty());
    }

    #[test]
    fn failed_re_extraction_keeps_prior_chunks_and_marks_stale() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let id = id_of(&root, "notes/paalala.md");
        let before = chunk_count(&conn, &id);
        fs::write(folder.path().join("notes/paalala.md"), [0xff, 0xfe, 0xfd, 0x00, 0x01]).unwrap();
        let summary = scan(&mut conn, &root);
        assert_eq!(summary.stale, 1);
        let (status, message) = status_of(&conn, &root, "notes/paalala.md");
        assert_eq!(status, "stale");
        assert!(message.unwrap().contains("previous version"));
        assert_eq!(chunk_count(&conn, &id), before);
        assert_eq!(search(&conn, &root.id, "checklist", 20).unwrap().iter().find(|hit| hit.document.id == id).unwrap().document.status, "stale");
    }

    #[test]
    fn new_unreadable_document_is_failed_not_indexed() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::write(folder.path().join("notes/broken.md"), [0xff, 0xfe]).unwrap();
        fs::write(folder.path().join("notes/broken.pdf"), b"%PDF-1.4 not really").unwrap();
        let summary = scan(&mut conn, &root);
        assert_eq!(summary.failed, 2);
        assert_eq!(status_of(&conn, &root, "notes/broken.md").0, "failed");
        assert_eq!(status_of(&conn, &root, "notes/broken.pdf").0, "failed");
    }

    #[test]
    fn hidden_folders_dependencies_and_symlinks_are_not_indexed() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::create_dir_all(folder.path().join(".git")).unwrap();
        fs::write(folder.path().join(".git/notes.md"), "hidden").unwrap();
        fs::create_dir_all(folder.path().join("node_modules/pkg")).unwrap();
        fs::write(folder.path().join("node_modules/pkg/readme.md"), "dependency").unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.md"), "outside secret").unwrap();
        #[cfg(unix)]
        let _ = std::os::unix::fs::symlink(outside.path().join("secret.md"), folder.path().join("link.md"));
        #[cfg(windows)]
        let _ = std::os::windows::fs::symlink_file(outside.path().join("secret.md"), folder.path().join("link.md"));
        scan(&mut conn, &root);
        let indexed: Vec<String> = list_documents(&conn, &root.id).unwrap().into_iter().map(|document| document.relative_path).collect();
        assert!(!indexed.iter().any(|path| path.contains(".git") || path.contains("node_modules") || path == "link.md"));
        assert!(search(&conn, &root.id, "secret", 5).unwrap().is_empty());
    }

    #[test]
    fn lost_folder_is_refused() {
        let (folder, mut conn, root) = fixture_workspace();
        drop(folder);
        let failure = scan_workspace(&mut conn, &root, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!(failure.code, ErrorCode::WorkspaceUnavailable);
    }

    #[test]
    fn cancellation_stops_without_losing_completed_work() {
        let (_folder, mut conn, root) = fixture_workspace();
        let summary = scan_workspace(&mut conn, &root, &AtomicBool::new(true), &mut |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.added, 0);
        let resumed = scan(&mut conn, &root);
        assert_eq!(resumed.added, resumed.total);
    }

    #[test]
    fn progress_reports_phases() {
        let (_folder, mut conn, root) = fixture_workspace();
        let mut phases = Vec::new();
        scan_workspace(&mut conn, &root, &AtomicBool::new(false), &mut |progress| phases.push(progress.phase)).unwrap();
        assert_eq!(phases.first(), Some(&"discovering"));
        assert!(phases.contains(&"indexing") && phases.contains(&"linking"));
        assert_eq!(phases.last(), Some(&"done"));
    }

    #[test]
    fn explicit_references_follow_the_frozen_relationship_shape() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let relationships = list_relationships(&conn, &root.id).unwrap();
        let plan = id_of(&root, "projects/project-plan.md");
        let notes = id_of(&root, "meetings/meeting-notes.md");
        let edge = relationships.iter().find(|edge| edge.source_id == notes && edge.target_id == plan).unwrap();
        assert_eq!(edge.link, LinkTarget { raw_target: "../projects/project-plan.md".into(), resolved_relative_path: "projects/project-plan.md".into() });
        assert_eq!(edge.evidence[0].text, "[project plan](../projects/project-plan.md)");
        assert_located(folder.path(), "meetings/meeting-notes.md", &edge.evidence[0]);
        let value = serde_json::to_value(edge).unwrap();
        assert_eq!((value["type"].as_str(), value["provenance"].as_str()), (Some("explicitReference"), Some("documentLink")));
        let copy = id_of(&root, "archive/project-plan-copy.md");
        let copy_targets: Vec<&String> = relationships.iter().filter(|edge| edge.source_id == copy).map(|edge| &edge.target_id).collect();
        assert_eq!(copy_targets, vec![&notes], "the copy's broken checklist link is not invented");
    }

    #[test]
    fn duplicates_are_verified_byte_identity() {
        let (folder, mut conn, root) = fixture_workspace();
        fs::write(folder.path().join("notes/near-copy.md"), fs::read_to_string(folder.path().join("projects/project-plan.md")).unwrap().replace("October 20", "October 21")).unwrap();
        scan(&mut conn, &root);
        let groups = duplicate_groups(&conn, &root).unwrap();
        assert_eq!(groups.len(), 1);
        let members: Vec<&str> = groups[0].documents.iter().map(|document| document.relative_path.as_str()).collect();
        assert_eq!(members, vec!["archive/project-plan-copy.md", "projects/project-plan.md"]);
        assert!(groups[0].content_hash.starts_with("sha256:"));
    }

    #[test]
    fn duplicate_group_is_not_reported_when_files_diverge_before_rescan() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        fs::write(folder.path().join("archive/project-plan-copy.md"), "changed outside Folio").unwrap();
        assert!(duplicate_groups(&conn, &root).unwrap().is_empty());
    }

    #[test]
    fn index_persists_across_restart() {
        let folder = tempfile::tempdir().unwrap();
        copy_fixtures(folder.path());
        let data = tempfile::tempdir().unwrap();
        let database = data.path().join("folio.sqlite");
        let workspace_id = {
            let mut conn = db::open(&database).unwrap();
            let root = authorize(&conn, folder.path());
            scan(&mut conn, &root);
            root.id
        };
        let mut conn = db::open(&database).unwrap();
        let mut registry = WorkspaceRegistry::new();
        let info = registry.authorize(&workspace::remembered_root(&conn, &workspace_id).unwrap()).unwrap();
        assert_eq!(info.id, workspace_id);
        let root = registry.resolve(&info.id).unwrap();
        assert!(!search(&conn, &root.id, "volunteer", 5).unwrap().is_empty());
        let again = scan(&mut conn, &root);
        assert_eq!(again.unchanged, again.total);
    }

    #[test]
    fn embedding_spaces_never_mix() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let space = |revision: &str| EmbeddingSpace { model_id: "multilingual-e5-small".into(), revision: revision.into(), quantization: "q8".into(), dimensions: 3, preprocessing_fingerprint: "passage-prefix-v1".into() };
        let old = register_space(&conn, &space("r1")).unwrap();
        let new = register_space(&conn, &space("r2")).unwrap();
        assert_eq!(old, "folio-space-v1/multilingual-e5-small/r1/q8/3/passage-prefix-v1");
        assert_ne!(old, new);
        assert_eq!(register_space(&conn, &space("r1")).unwrap(), old);
        let pending = pending_embedding_chunks(&conn, &root.id, &old, 2).unwrap();
        put_embeddings(&mut conn, &root.id, &old, &[ChunkVector { chunk_id: pending[0].chunk_id, content_hash: pending[0].content_hash.clone(), vector: vec![1.0, 0.0, 0.0] }]).unwrap();
        put_embeddings(&mut conn, &root.id, &new, &[ChunkVector { chunk_id: pending[1].chunk_id, content_hash: pending[1].content_hash.clone(), vector: vec![1.0, 0.0, 0.0] }]).unwrap();
        let from_old = vector_candidates(&conn, &root.id, &old, &[1.0, 0.0, 0.0], 10).unwrap();
        assert_eq!(from_old.iter().map(|candidate| candidate.chunk_id).collect::<Vec<_>>(), vec![pending[0].chunk_id]);
        assert_eq!(from_old[0].space_fingerprint, old);
        assert_eq!(pending_embedding_chunks(&conn, &root.id, &new, 1000).unwrap().iter().filter(|chunk| chunk.chunk_id == pending[0].chunk_id).count(), 1);
        let mismatch = put_embeddings(&mut conn, &root.id, &old, &[ChunkVector { chunk_id: pending[0].chunk_id, content_hash: pending[0].content_hash.clone(), vector: vec![1.0; 4] }]).unwrap_err();
        assert_eq!(mismatch.code, ErrorCode::EmbeddingSpaceMismatch);
        assert_eq!(vector_candidates(&conn, &root.id, &old, &[1.0; 4], 3).unwrap_err().code, ErrorCode::EmbeddingSpaceMismatch);
    }

    #[test]
    fn edits_invalidate_embeddings_for_changed_chunks() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let space = register_space(&conn, &EmbeddingSpace { model_id: "m".into(), revision: "1".into(), quantization: "q".into(), dimensions: 2, preprocessing_fingerprint: "p".into() }).unwrap();
        let plan_id = id_of(&root, "projects/project-plan.md");
        let (chunk_id, content_hash): (i64, String) = conn.query_row("SELECT chunk_id, content_hash FROM chunks WHERE document_id = ?1", [&plan_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        put_embeddings(&mut conn, &root.id, &space, &[ChunkVector { chunk_id, content_hash, vector: vec![0.5, 0.5] }]).unwrap();
        fs::write(folder.path().join("projects/project-plan.md"), "# Plan\n\nRewritten content with a different length.").unwrap();
        scan(&mut conn, &root);
        let vectors: i64 = conn.query_row("SELECT count(*) FROM embeddings", [], |row| row.get(0)).unwrap();
        assert_eq!(vectors, 0);
    }

    // TJ's repros from the PR #11 review, plus cross-platform versions of the Unix ones.
    #[cfg(unix)]
    #[test]
    fn stale_document_is_retried_once_readable() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("a.md");
        fs::write(&file, "first version").unwrap();
        let mut conn = db::open_in_memory().unwrap();
        let root = authorize(&conn, folder.path());
        scan(&mut conn, &root);
        fs::write(&file, "second version, longer").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(scan(&mut conn, &root).stale, 1);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        scan(&mut conn, &root);
        assert_eq!(status_of(&conn, &root, "a.md").0, "indexed");
    }

    /// A read that failed and later succeeds on a file with the same size and modification
    /// time, as when a lock is released or a cloud placeholder finishes downloading.
    fn becomes_readable_with_same_metadata(path: &Path, unreadable: &[u8], readable: &[u8]) {
        assert_eq!(unreadable.len(), readable.len());
        let modified = fs::metadata(path).unwrap().modified().unwrap();
        fs::write(path, readable).unwrap();
        fs::File::options().write(true).open(path).unwrap().set_modified(modified).unwrap();
    }

    #[test]
    fn stale_and_failed_documents_are_retried_even_when_size_and_time_match() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let stale_file = folder.path().join("notes/paalala.md");
        let broken = [0xffu8, 0xfe, 0xfd, b'x', b'y', b'z'];
        fs::write(&stale_file, broken).unwrap();
        let failed_file = folder.path().join("notes/bago.md");
        fs::write(&failed_file, broken).unwrap();
        let first = scan(&mut conn, &root);
        assert_eq!((first.stale, first.failed), (1, 1));
        becomes_readable_with_same_metadata(&stale_file, &broken, b"ayos..");
        becomes_readable_with_same_metadata(&failed_file, &broken, b"bago..");
        scan(&mut conn, &root);
        assert_eq!(status_of(&conn, &root, "notes/paalala.md").0, "indexed");
        assert_eq!(status_of(&conn, &root, "notes/bago.md").0, "indexed");
        assert_eq!(paths(&search(&conn, &root.id, "ayos", 5).unwrap()), vec!["notes/paalala.md"]);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_subfolder_is_not_treated_as_deleted() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        fs::create_dir(folder.path().join("sub")).unwrap();
        fs::write(folder.path().join("sub/a.md"), "keep me").unwrap();
        let mut conn = db::open_in_memory().unwrap();
        let root = authorize(&conn, folder.path());
        scan(&mut conn, &root);
        fs::set_permissions(folder.path().join("sub"), fs::Permissions::from_mode(0o000)).unwrap();
        let summary = scan(&mut conn, &root);
        fs::set_permissions(folder.path().join("sub"), fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(summary.removed, 0);
        assert_eq!(paths(&search(&conn, &root.id, "keep", 5).unwrap()), vec!["sub/a.md"], "its index data is kept");
    }

    #[test]
    fn records_inside_unreadable_locations_are_never_removed() {
        let existing: Vec<String> = ["sub/a.md", "sub/deeper/b.md", "subway.md", "gone.md", "kept.md"].iter().map(|path| path.to_string()).collect();
        let present: HashSet<&str> = ["kept.md"].into_iter().collect();
        let discovery = Discovery { found: Vec::new(), skipped: 1, unreadable: vec!["sub".into()], unreadable_unknown: false };
        assert_eq!(removed_paths(existing.iter(), &present, &discovery), vec!["subway.md".to_string(), "gone.md".to_string()]);
        let unknown = Discovery { unreadable_unknown: true, ..discovery };
        assert!(removed_paths(existing.iter(), &present, &unknown).is_empty(), "a failure with no location removes nothing");
    }

    #[test]
    fn a_folder_over_the_document_limit_is_refused_before_anything_is_written() {
        let folder = tempfile::tempdir().unwrap();
        for index in 0..=MAX_DOCUMENTS { fs::write(folder.path().join(format!("n{index}.txt")), "x").unwrap(); }
        let mut conn = db::open_in_memory().unwrap();
        let root = authorize(&conn, folder.path());
        let failure = scan_workspace(&mut conn, &root, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!((failure.code, failure.detail("reason")), (ErrorCode::WorkspaceUnavailable, Some("tooManyDocuments")));
        assert!(list_documents(&conn, &root.id).unwrap().is_empty());
    }

    // TJ's repro from the PR #11 review: a reused chunk id must not accept a vector
    // computed for the text it held before the rescan.
    #[test]
    fn stale_vector_is_rejected_after_rescan() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("a.md"), "alpha old text").unwrap();
        let mut conn = db::open_in_memory().unwrap();
        let root = authorize(&conn, folder.path());
        scan(&mut conn, &root);
        let space = register_space(&conn, &EmbeddingSpace { model_id: "m".into(), revision: "1".into(), quantization: "q".into(), dimensions: 2, preprocessing_fingerprint: "p".into() }).unwrap();
        let pending = pending_embedding_chunks(&conn, &root.id, &space, 10).unwrap().remove(0);
        fs::write(folder.path().join("a.md"), "bravo new and longer text").unwrap();
        scan(&mut conn, &root);
        let reused: i64 = conn.query_row("SELECT count(*) FROM chunks WHERE chunk_id = ?1", [pending.chunk_id], |row| row.get(0)).unwrap();
        assert_eq!(reused, 1, "the id was reused for the new text, which is what makes this dangerous");
        let accepted = put_embeddings(&mut conn, &root.id, &space, &[ChunkVector { chunk_id: pending.chunk_id, content_hash: pending.content_hash, vector: vec![1.0, 0.0] }]);
        assert_eq!(accepted.unwrap_err().detail("reason"), Some("chunkChanged"));
        let refetched = pending_embedding_chunks(&conn, &root.id, &space, 10).unwrap().remove(0);
        assert!(refetched.text.starts_with("bravo"));
        put_embeddings(&mut conn, &root.id, &space, &[ChunkVector { chunk_id: refetched.chunk_id, content_hash: refetched.content_hash, vector: vec![1.0, 0.0] }]).unwrap();
    }

    #[test]
    fn linked_path_matches_frontend_rules() {
        assert_eq!(linked_path("projects/project-plan.md", "../meetings/meeting-notes.md").as_deref(), Some("meetings/meeting-notes.md"));
        assert_eq!(linked_path("a.md", "../outside.md"), None);
        assert_eq!(linked_path("a/b.md", "https://example.com"), None);
        assert_eq!(linked_path("a/b.md", "/etc/passwd"), None);
        assert_eq!(linked_path("a/b.md", "my%20notes.md#part").as_deref(), Some("a/my notes.md"));
        assert_eq!(linked_path("a/b.md", "%E0%A4%A"), None);
    }
}
