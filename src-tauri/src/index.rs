use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;
use crate::error::{fail, ErrorCode, NativeResult};
use crate::extract::{self, Extraction, MediaKind, Utf16Cursor};
use crate::workspace::{self, now_millis, ScopedRoot};

const MAX_DOCUMENTS: usize = 5000;
const BATCH_SIZE: usize = 50;
const EXCERPT_BEFORE: usize = 120;
const EXCERPT_LENGTH: usize = 360;
const MAX_QUERY_TERMS: usize = 32;
const PASSAGES_PER_RESULT: usize = 3;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexedDocument {
    pub id: String,
    pub relative_path: String,
    pub name: String,
    pub title: String,
    /// Language detection belongs to the provider track; the index does not guess.
    pub language: &'static str,
    pub size_bytes: i64,
    pub content_hash: String,
    pub media_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    pub modified_at: String,
    pub indexed_at: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourcePassage {
    pub document_id: String,
    /// UTF-16 code-unit offsets into the document's extracted text.
    pub start: usize,
    pub end: usize,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub page: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Relationship {
    pub source_id: String,
    pub target_id: String,
    #[serde(rename = "type")]
    pub relationship_type: String,
    pub evidence: Vec<SourcePassage>,
    pub provenance: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    pub source_content_hash: String,
    pub target_content_hash: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
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
    pub skipped: usize,
    pub cancelled: bool,
    pub duration_ms: u64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateGroup {
    pub content_hash: String,
    pub size_bytes: i64,
    /// Every listed document was re-read and compared byte for byte.
    pub documents: Vec<IndexedDocument>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
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
    Ok(hex::encode(hasher.finalize()))
}

const DOCUMENT_COLUMNS: &str = "id, relative_path, name, COALESCE(title, name), size_bytes, content_hash, media_type, status, status_message, modified_at, indexed_at";

fn document_from_row(row: &Row<'_>) -> rusqlite::Result<IndexedDocument> {
    Ok(IndexedDocument {
        id: row.get(0)?,
        relative_path: row.get(1)?,
        name: row.get(2)?,
        title: row.get(3)?,
        language: "unknown",
        size_bytes: row.get(4)?,
        content_hash: row.get(5)?,
        media_type: row.get(6)?,
        status: row.get(7)?,
        status_message: row.get(8)?,
        modified_at: row.get(9)?,
        indexed_at: row.get(10)?,
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
        .ok_or_else(|| fail(ErrorCode::NotFound, "The document is not in this workspace's index."))
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

fn discover(root: &Path) -> NativeResult<(Vec<Found>, usize)> {
    let mut found = Vec::new();
    let mut skipped = 0;
    let walker = WalkDir::new(root).follow_links(false).into_iter().filter_entry(|entry| entry.depth() == 0 || !(entry.file_type().is_dir() && skipped_directory(entry.file_name())));
    for entry in walker {
        let Ok(entry) = entry else {
            skipped += 1;
            continue;
        };
        // Symlinks report their own type here, so links never enter the index.
        if !entry.file_type().is_file() || entry.file_name().to_string_lossy().starts_with('.') { continue; }
        let Some(kind) = MediaKind::from_path(entry.path()) else { continue };
        let Ok(metadata) = entry.metadata() else {
            skipped += 1;
            continue;
        };
        let relative = entry.path().strip_prefix(root).map_err(|_| fail(ErrorCode::PathEscape, "A document path escaped the folder."))?.to_string_lossy().replace('\\', "/");
        let modified = metadata.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|elapsed| elapsed.as_nanos().to_string()).unwrap_or_default();
        found.push(Found { relative, path: entry.path().to_path_buf(), kind, size: metadata.len() as i64, modified });
        if found.len() > MAX_DOCUMENTS {
            return Err(fail(ErrorCode::TooLarge, "Folio supports up to 5,000 TXT, Markdown and PDF documents per folder. Choose a smaller folder."));
        }
    }
    found.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok((found, skipped))
}

/// Incrementally indexes the authorized folder. Unchanged files are not re-read; changed
/// files have their chunks, vectors, relationships and caches replaced; deleted files are
/// removed. Cancellation keeps all completed batches.
pub fn scan_workspace(conn: &mut Connection, root: &ScopedRoot, cancel: &AtomicBool, progress: &mut dyn FnMut(&IndexProgress)) -> NativeResult<ScanSummary> {
    let started = std::time::Instant::now();
    let root_path = workspace::available_root(&root.path)?;
    let report = |phase, processed, total, current_path| IndexProgress { workspace_id: root.id.clone(), phase, processed, total, current_path };
    progress(&report("discovering", 0, 0, None));
    let (found, skipped) = discover(&root_path)?;
    let mut summary = ScanSummary { workspace_id: root.id.clone(), total: found.len(), skipped, ..Default::default() };

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
    let removed: Vec<&Existing> = existing.iter().filter(|(path, _)| !present.contains(path.as_str())).map(|(_, record)| record).collect();
    if !removed.is_empty() {
        let tx = conn.transaction()?;
        for record in &removed { forget_document(&tx, &record.id)?; }
        tx.commit()?;
        summary.removed = removed.len();
    }

    let mut processed = 0;
    'batches: for batch in found.chunks(BATCH_SIZE) {
        let tx = conn.transaction()?;
        for file in batch {
            if cancel.load(Ordering::SeqCst) {
                summary.cancelled = true;
                tx.commit()?;
                break 'batches;
            }
            match index_file(&tx, &root.id, file, existing.get(&file.relative))? {
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

fn index_file(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>) -> NativeResult<Outcome> {
    if let Some(prior) = prior {
        if prior.size == file.size && prior.modified == file.modified { return Ok(Outcome::Unchanged); }
    }
    let bytes = match workspace::read_bounded(&file.path, file.kind.max_bytes()) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.code == ErrorCode::TooLarge => None,
        Err(error) => return record_failure(tx, workspace_id, file, prior, &error.message, None),
    };
    let hash = match &bytes {
        Some(bytes) => sha256_hex(bytes),
        None => match sha256_file(&file.path) {
            Ok(hash) => hash,
            Err(error) => return record_failure(tx, workspace_id, file, prior, &error.message, None),
        },
    };
    if let Some(prior) = prior {
        if prior.hash == hash {
            // Same bytes as indexed: refresh metadata only. A stale record whose file returned
            // to its indexed content is current again.
            tx.execute(
                "UPDATE documents SET size_bytes = ?1, modified_at = ?2, status = CASE status WHEN 'stale' THEN 'indexed' ELSE status END, status_message = CASE status WHEN 'stale' THEN NULL ELSE status_message END WHERE id = ?3",
                params![file.size, file.modified, prior.id],
            )?;
            return Ok(Outcome::Unchanged);
        }
    }
    let Some(bytes) = bytes else {
        let reason = format!("Larger than the {} MiB limit for this file type.", file.kind.max_bytes() / 1024 / 1024);
        return record_unsupported(tx, workspace_id, file, prior, &hash, &reason);
    };
    match extract::extract(file.kind, &bytes) {
        Ok(Extraction::Text(extracted)) => {
            let title = extract::title_of(&extracted.text);
            let id = upsert_document(tx, workspace_id, file, prior, &hash, title.as_deref(), "indexed", None, true)?;
            clear_derived(tx, &id)?;
            let mut insert = tx.prepare_cached("INSERT INTO chunks (document_id, ordinal, chunk_text, start_offset, end_offset, page, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)")?;
            for chunk in extract::chunk(&extracted) {
                insert.execute(params![id, chunk.ordinal as i64, chunk.text, chunk.start as i64, chunk.end as i64, chunk.page, sha256_hex(chunk.text.as_bytes())])?;
            }
            Ok(if prior.is_some() { Outcome::Updated } else { Outcome::Added })
        }
        Ok(Extraction::Unsupported(reason)) => record_unsupported(tx, workspace_id, file, prior, &hash, &reason),
        Err(error) => record_failure(tx, workspace_id, file, prior, &error.message, Some(&hash)),
    }
}

#[allow(clippy::too_many_arguments)]
fn upsert_document(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>, hash: &str, title: Option<&str>, status: &str, message: Option<&str>, indexed: bool) -> NativeResult<String> {
    let name = file.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let indexed_at = indexed.then(now_millis);
    match prior {
        Some(prior) => {
            tx.execute(
                "UPDATE documents SET name = ?1, title = ?2, content_hash = ?3, media_type = ?4, size_bytes = ?5, modified_at = ?6, indexed_at = COALESCE(?7, indexed_at), status = ?8, status_message = ?9 WHERE id = ?10",
                params![name, title, hash, file.kind.media_type(), file.size, file.modified, indexed_at, status, message, prior.id],
            )?;
            Ok(prior.id.clone())
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO documents (id, workspace_id, relative_path, name, title, content_hash, media_type, size_bytes, modified_at, indexed_at, status, status_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![id, workspace_id, file.relative, name, title, hash, file.kind.media_type(), file.size, file.modified, indexed_at, status, message],
            )?;
            Ok(id)
        }
    }
}

fn record_unsupported(tx: &Transaction<'_>, workspace_id: &str, file: &Found, prior: Option<&Existing>, hash: &str, reason: &str) -> NativeResult<Outcome> {
    let id = upsert_document(tx, workspace_id, file, prior, hash, None, "unsupported", Some(reason), false)?;
    clear_derived(tx, &id)?;
    Ok(Outcome::Unsupported)
}

/// Extraction failure never erases valid prior state: an indexed document keeps its chunks
/// and becomes `stale`; a document with no usable prior index is recorded as `failed`.
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

fn forget_document(tx: &Transaction<'_>, document_id: &str) -> NativeResult<()> {
    // History survives the document so applied changes stay recoverable.
    tx.execute("UPDATE history SET document_id = NULL WHERE document_id = ?1", [document_id])?;
    tx.execute("DELETE FROM documents WHERE id = ?1", [document_id])?;
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

/// Mirrors `linkedPath` in `src/domain/discovery.ts`: relative links only, never above the root.
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
    Some(parts.join("/"))
}

/// Rebuilds Markdown-link relationships for the workspace, with the link text as evidence.
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
    let mut edges: Vec<((String, String), Vec<SourcePassage>)> = Vec::new();
    {
        let mut statement = tx.prepare(
            "SELECT d.id, d.relative_path, c.chunk_text, c.start_offset, c.page FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.workspace_id = ?1 AND c.chunk_text LIKE '%](%' ORDER BY d.relative_path, c.ordinal",
        )?;
        let rows = statement.query_map([workspace_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, Option<u32>>(4)?)))?;
        for row in rows {
            let (source_id, source_path, text, chunk_start, page) = row?;
            let mut cursor = Utf16Cursor::new(&text);
            for link in extract::markdown_links(&text) {
                let Some((target_id, _)) = linked_path(&source_path, &link.target).and_then(|path| by_path.get(&path)) else { continue };
                if *target_id == source_id { continue; }
                let passage = SourcePassage {
                    document_id: source_id.clone(),
                    start: chunk_start as usize + cursor.at(link.whole.start),
                    end: chunk_start as usize + cursor.at(link.whole.end),
                    text: text[link.whole.clone()].to_owned(),
                    page,
                };
                let key = (source_id.clone(), target_id.clone());
                match edges.iter_mut().find(|(existing, _)| *existing == key) {
                    Some((_, evidence)) => evidence.push(passage),
                    None => edges.push((key, vec![passage])),
                }
            }
        }
    }
    let hash_of: HashMap<&String, &String> = by_path.values().map(|(id, hash)| (id, hash)).collect();
    let now = now_millis();
    for ((source, target), evidence) in &edges {
        tx.execute(
            "INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES (?1, ?2, ?3, 'explicitReference', ?4, 'documentLink', NULL, ?5, ?6, ?7)",
            params![uuid::Uuid::new_v4().to_string(), source, target, serde_json::to_string(evidence)?, hash_of[source], hash_of[target], now],
        )?;
    }
    Ok(edges.len())
}

pub fn list_relationships(conn: &Connection, workspace_id: &str) -> NativeResult<Vec<Relationship>> {
    let mut statement = conn.prepare(
        "SELECT r.source_document_id, r.target_document_id, r.relationship_type, r.evidence_json, r.provenance, r.confidence, r.source_content_hash, r.target_content_hash FROM relationships r JOIN documents d ON d.id = r.source_document_id WHERE d.workspace_id = ?1 ORDER BY d.relative_path",
    )?;
    let rows = statement.query_map([workspace_id], |row| {
        Ok(Relationship {
            source_id: row.get(0)?,
            target_id: row.get(1)?,
            relationship_type: row.get(2)?,
            evidence: serde_json::from_str(&row.get::<_, String>(3)?).unwrap_or_default(),
            provenance: row.get(4)?,
            confidence: row.get(5)?,
            source_content_hash: row.get(6)?,
            target_content_hash: row.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
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

/// A window of the chunk around the first query term, with UTF-16 document offsets.
fn excerpt(document_id: &str, chunk_text: &str, chunk_start: usize, page: Option<u32>, terms: &[String]) -> SourcePassage {
    let lower = chunk_text.to_lowercase();
    let hit = if lower.len() == chunk_text.len() { terms.iter().filter_map(|term| find_word(&lower, term)).min().unwrap_or(0) } else { 0 };
    let boundaries: Vec<usize> = chunk_text.char_indices().map(|(index, _)| index).chain(std::iter::once(chunk_text.len())).collect();
    let hit_char = boundaries.partition_point(|&index| index < hit);
    let from_char = hit_char.saturating_sub(EXCERPT_BEFORE);
    let to_char = (from_char + EXCERPT_LENGTH).min(boundaries.len() - 1);
    let (from, to) = (boundaries[from_char], boundaries[to_char]);
    let mut cursor = Utf16Cursor::new(chunk_text);
    SourcePassage { document_id: document_id.to_owned(), start: chunk_start + cursor.at(from), end: chunk_start + cursor.at(to), text: chunk_text[from..to].to_owned(), page }
}

/// FTS5 keyword search over indexed chunks. Query words are quoted and OR-ed, so document
/// or query text cannot inject FTS syntax. This is keyword matching, not semantic search.
pub fn search(conn: &Connection, workspace_id: &str, query: &str, limit: usize) -> NativeResult<Vec<SearchHit>> {
    let terms = query_terms(query);
    if terms.is_empty() { return Ok(Vec::new()); }
    let expression = terms.iter().map(|term| format!("\"{term}\"")).collect::<Vec<_>>().join(" OR ");
    let limit = limit.clamp(1, 100);
    let mut statement = conn.prepare(
        "SELECT c.document_id, c.chunk_text, c.start_offset, c.page, bm25(chunks_fts) AS rank FROM chunks_fts JOIN chunks c ON c.chunk_id = chunks_fts.rowid JOIN documents d ON d.id = c.document_id WHERE chunks_fts MATCH ?1 AND d.workspace_id = ?2 ORDER BY rank LIMIT ?3",
    )?;
    let rows = statement.query_map(params![expression, workspace_id, (limit * 8).min(400) as i64], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, Option<u32>>(3)?, row.get::<_, f64>(4)?))
    })?;
    let mut grouped: Vec<(String, f64, Vec<SourcePassage>)> = Vec::new();
    for row in rows {
        let (document_id, text, start, page, rank) = row?;
        let passage = excerpt(&document_id, &text, start as usize, page, &terms);
        match grouped.iter().position(|(id, _, _)| *id == document_id) {
            Some(index) if grouped[index].2.len() < PASSAGES_PER_RESULT => grouped[index].2.push(passage),
            Some(_) => {}
            None if grouped.len() < limit => grouped.push((document_id, -rank, vec![passage])),
            None => {}
        }
    }
    grouped
        .into_iter()
        .map(|(document_id, score, passages)| Ok(SearchHit { document: get_document(conn, workspace_id, &document_id)?, passages, score, method: "keyword" }))
        .collect()
}

// ---------------------------------------------------------------- exact duplicates

/// Groups documents whose bytes are identical. The stored hash only nominates candidates;
/// each group is confirmed by re-reading and comparing the files.
pub fn duplicate_groups(conn: &Connection, root: &ScopedRoot) -> NativeResult<Vec<DuplicateGroup>> {
    let mut statement = conn.prepare(
        "SELECT content_hash FROM documents WHERE workspace_id = ?1 AND content_hash != '' AND status IN ('indexed','unsupported') GROUP BY content_hash HAVING count(*) > 1 ORDER BY content_hash",
    )?;
    let hashes: Vec<String> = statement.query_map([&root.id], |row| row.get(0))?.collect::<Result<_, _>>()?;
    let mut groups = Vec::new();
    for hash in hashes {
        let mut members = conn.prepare(&format!("SELECT {DOCUMENT_COLUMNS} FROM documents WHERE workspace_id = ?1 AND content_hash = ?2 ORDER BY relative_path"))?;
        let documents: Vec<IndexedDocument> = members.query_map([&root.id, &hash], document_from_row)?.collect::<Result<_, _>>()?;
        let mut confirmed: Vec<(Vec<u8>, Vec<IndexedDocument>)> = Vec::new();
        for document in documents {
            let Ok(path) = workspace::resolve_document(&root.path, &document.relative_path) else { continue };
            let Ok(bytes) = workspace::read_bounded(&path, extract::MAX_PDF_BYTES) else { continue };
            match confirmed.iter_mut().find(|(existing, _)| *existing == bytes) {
                Some((_, members)) => members.push(document),
                None => confirmed.push((bytes, vec![document])),
            }
        }
        for (bytes, documents) in confirmed {
            if documents.len() > 1 && sha256_hex(&bytes) == hash {
                groups.push(DuplicateGroup { content_hash: hash.clone(), size_bytes: bytes.len() as i64, documents });
            }
        }
    }
    Ok(groups)
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
    pub vector: Vec<f32>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PendingChunk {
    pub chunk_id: i64,
    pub document_id: String,
    pub text: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VectorCandidate {
    pub chunk_id: i64,
    pub score: f32,
    pub passage: SourcePassage,
}

/// Each distinct model/revision/quantization/dimension/preprocessing tuple is its own space.
pub fn register_space(conn: &Connection, space: &EmbeddingSpace) -> NativeResult<String> {
    let fields = [&space.model_id, &space.revision, &space.quantization, &space.preprocessing_fingerprint];
    if fields.iter().any(|field| field.trim().is_empty()) || space.dimensions == 0 || space.dimensions > 8192 {
        return Err(fail(ErrorCode::InvalidInput, "An embedding space needs a model, revision, quantization, preprocessing fingerprint and 1–8192 dimensions."));
    }
    let fingerprint = format!("{}\0{}\0{}\0{}\0{}", space.model_id, space.revision, space.quantization, space.dimensions, space.preprocessing_fingerprint);
    let id = format!("space-{}", &sha256_hex(fingerprint.as_bytes())[..32]);
    conn.execute(
        "INSERT OR IGNORE INTO embedding_spaces (id, model_id, revision, quantization, dimensions, preprocessing_fingerprint) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, space.model_id, space.revision, space.quantization, space.dimensions, space.preprocessing_fingerprint],
    )?;
    Ok(id)
}

fn space_dimensions(conn: &Connection, space_id: &str) -> NativeResult<usize> {
    conn.query_row("SELECT dimensions FROM embedding_spaces WHERE id = ?1", [space_id], |row| row.get::<_, i64>(0))
        .optional()?
        .map(|dimensions| dimensions as usize)
        .ok_or_else(|| fail(ErrorCode::NotFound, "Unknown embedding space. Register it first."))
}

fn check_vector(vector: &[f32], dimensions: usize) -> NativeResult<()> {
    if vector.len() != dimensions {
        return Err(fail(ErrorCode::EmbeddingSpaceMismatch, format!("Expected a {dimensions}-dimension vector for this embedding space, got {}.", vector.len())));
    }
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(fail(ErrorCode::InvalidInput, "Vectors must contain finite numbers."));
    }
    Ok(())
}

pub fn put_embeddings(conn: &mut Connection, workspace_id: &str, space_id: &str, items: &[ChunkVector]) -> NativeResult<usize> {
    let dimensions = space_dimensions(conn, space_id)?;
    let tx = conn.transaction()?;
    for item in items {
        check_vector(&item.vector, dimensions)?;
        let owned: Option<i64> = tx
            .query_row("SELECT c.chunk_id FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.chunk_id = ?1 AND d.workspace_id = ?2", params![item.chunk_id, workspace_id], |row| row.get(0))
            .optional()?;
        if owned.is_none() { return Err(fail(ErrorCode::NotFound, format!("Chunk {} is not in this workspace's current index.", item.chunk_id))); }
        let blob: Vec<u8> = item.vector.iter().flat_map(|value| value.to_le_bytes()).collect();
        tx.execute("INSERT OR REPLACE INTO embeddings (chunk_id, space_id, vector) VALUES (?1, ?2, ?3)", params![item.chunk_id, space_id, blob])?;
    }
    tx.commit()?;
    Ok(items.len())
}

/// Chunks that have no vector in this space yet, for the embedding provider to process.
pub fn pending_embedding_chunks(conn: &Connection, workspace_id: &str, space_id: &str, limit: usize) -> NativeResult<Vec<PendingChunk>> {
    space_dimensions(conn, space_id)?;
    let mut statement = conn.prepare(
        "SELECT c.chunk_id, c.document_id, c.chunk_text FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.workspace_id = ?1 AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.chunk_id = c.chunk_id AND e.space_id = ?2) ORDER BY c.chunk_id LIMIT ?3",
    )?;
    let rows = statement.query_map(params![workspace_id, space_id, limit.clamp(1, 512) as i64], |row| Ok(PendingChunk { chunk_id: row.get(0)?, document_id: row.get(1)?, text: row.get(2)? }))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Exact cosine search within one embedding space. Vectors from other spaces are never read.
pub fn vector_candidates(conn: &Connection, workspace_id: &str, space_id: &str, query: &[f32], k: usize) -> NativeResult<Vec<VectorCandidate>> {
    check_vector(query, space_dimensions(conn, space_id)?)?;
    let query_norm = query.iter().map(|value| value * value).sum::<f32>().sqrt();
    let mut statement = conn.prepare(
        "SELECT e.chunk_id, c.document_id, c.chunk_text, c.start_offset, c.end_offset, c.page, e.vector FROM embeddings e JOIN chunks c ON c.chunk_id = e.chunk_id JOIN documents d ON d.id = c.document_id WHERE e.space_id = ?1 AND d.workspace_id = ?2",
    )?;
    let rows = statement.query_map(params![space_id, workspace_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, Option<u32>>(5)?, row.get::<_, Vec<u8>>(6)?))
    })?;
    let mut candidates = Vec::new();
    for row in rows {
        let (chunk_id, document_id, text, start, end, page, blob) = row?;
        let vector: Vec<f32> = blob.chunks_exact(4).map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
        if vector.len() != query.len() { continue; }
        let dot: f32 = vector.iter().zip(query).map(|(a, b)| a * b).sum();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        let score = if norm == 0.0 || query_norm == 0.0 { 0.0 } else { dot / (norm * query_norm) };
        candidates.push(VectorCandidate { chunk_id, score, passage: SourcePassage { document_id, start: start as usize, end: end as usize, text, page } });
    }
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    candidates.truncate(k.clamp(1, 100));
    Ok(candidates)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{db, extract::testpdf, workspace::remember_picked_folder};
    use std::fs;

    pub fn copy_fixtures(destination: &Path) {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/documents");
        for entry in WalkDir::new(&source) {
            let entry = entry.unwrap();
            let target = destination.join(entry.path().strip_prefix(&source).unwrap());
            if entry.file_type().is_dir() { fs::create_dir_all(&target).unwrap(); } else { fs::copy(entry.path(), &target).unwrap(); }
        }
    }

    pub fn scan(conn: &mut Connection, root: &ScopedRoot) -> ScanSummary {
        scan_workspace(conn, root, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    pub fn fixture_workspace() -> (tempfile::TempDir, Connection, ScopedRoot) {
        let folder = tempfile::tempdir().unwrap();
        copy_fixtures(folder.path());
        let conn = db::open_in_memory().unwrap();
        let root = remember_picked_folder(&conn, folder.path()).unwrap();
        (folder, conn, root)
    }

    pub fn id_of(conn: &Connection, root: &ScopedRoot, path: &str) -> String {
        conn.query_row("SELECT id FROM documents WHERE workspace_id = ?1 AND relative_path = ?2", [&root.id, path], |row| row.get(0)).unwrap()
    }

    fn status_of(conn: &Connection, root: &ScopedRoot, path: &str) -> (String, Option<String>) {
        conn.query_row("SELECT status, status_message FROM documents WHERE workspace_id = ?1 AND relative_path = ?2", [&root.id, path], |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }

    fn chunk_count(conn: &Connection, document_id: &str) -> i64 {
        conn.query_row("SELECT count(*) FROM chunks WHERE document_id = ?1", [document_id], |row| row.get(0)).unwrap()
    }

    fn paths(hits: &[SearchHit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.document.relative_path.as_str()).collect()
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
        assert_eq!(hit.document.id, id_of(&conn, &root, "notes/tala-sa-proyekto.md"));
        let passage = &hit.passages[0];
        assert!(passage.text.contains("huling araw ng pagpasa"));
        let content = fs::read_to_string(folder.path().join("notes/tala-sa-proyekto.md")).unwrap();
        let units: Vec<u16> = content.encode_utf16().collect();
        assert_eq!(String::from_utf16(&units[passage.start..passage.end]).unwrap(), passage.text);
    }

    #[test]
    fn excerpt_offsets_survive_non_ascii_text() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let hits = search(&conn, &root.id, "authoritative schedule", 5).unwrap();
        assert_eq!(hits[0].document.relative_path, "projects/submission-checklist.md");
        let content = fs::read_to_string(folder.path().join("projects/submission-checklist.md")).unwrap();
        let units: Vec<u16> = content.encode_utf16().collect();
        let passage = &hits[0].passages[0];
        assert_eq!(String::from_utf16(&units[passage.start..passage.end]).unwrap(), passage.text);
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
        let plan_id = id_of(&conn, &root, "projects/project-plan.md");
        let notes_id = id_of(&conn, &root, "meetings/meeting-notes.md");
        let relationship_count = |conn: &Connection, id: &str| -> i64 { conn.query_row("SELECT count(*) FROM relationships WHERE source_document_id = ?1", [id], |row| row.get(0)).unwrap() };
        assert_eq!(relationship_count(&conn, &plan_id), 2);
        conn.execute("INSERT INTO derived_cache (document_id, kind, content_hash, payload, created_at) VALUES (?1, 'summary', 'x', 'old summary', '0')", [&plan_id]).unwrap();

        fs::write(folder.path().join("projects/project-plan.md"), "# Community Learning Project\n\nThe deadline moved to November 3. No links remain.\n").unwrap();
        fs::remove_file(folder.path().join("personal/grocery-list.md")).unwrap();
        let summary = scan(&mut conn, &root);
        assert_eq!((summary.updated, summary.removed), (1, 1));
        assert_eq!(id_of(&conn, &root, "projects/project-plan.md"), plan_id, "identity survives an edit");
        assert_eq!(paths(&search(&conn, &root.id, "November", 5).unwrap()), vec!["projects/project-plan.md"]);
        assert!(!paths(&search(&conn, &root.id, "volunteer", 10).unwrap()).contains(&"projects/project-plan.md"));
        assert_eq!(relationship_count(&conn, &plan_id), 0, "links removed from the edited file are dropped");
        assert_eq!(relationship_count(&conn, &notes_id), 1, "links into the edited file are rebuilt");
        let cached: i64 = conn.query_row("SELECT count(*) FROM derived_cache WHERE document_id = ?1", [&plan_id], |row| row.get(0)).unwrap();
        assert_eq!(cached, 0, "cached summary invalidated");
        let grocery: i64 = conn.query_row("SELECT count(*) FROM documents WHERE relative_path = 'personal/grocery-list.md'", [], |row| row.get(0)).unwrap();
        assert_eq!(grocery, 0);
        assert!(search(&conn, &root.id, "grocery", 5).unwrap().is_empty());
    }

    #[test]
    fn failed_re_extraction_keeps_prior_chunks_and_marks_stale() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let id = id_of(&conn, &root, "notes/paalala.md");
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
        let error = scan_workspace(&mut conn, &root, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
        assert_eq!(error.code, ErrorCode::NotAuthorized);
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
    fn explicit_references_carry_located_link_evidence() {
        let (_folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let relationships = list_relationships(&conn, &root.id).unwrap();
        let plan = id_of(&conn, &root, "projects/project-plan.md");
        let notes = id_of(&conn, &root, "meetings/meeting-notes.md");
        let edge = relationships.iter().find(|edge| edge.source_id == notes && edge.target_id == plan).unwrap();
        assert_eq!((edge.relationship_type.as_str(), edge.provenance.as_str()), ("explicitReference", "documentLink"));
        assert_eq!(edge.evidence[0].text, "[project plan](../projects/project-plan.md)");
        let copy = id_of(&conn, &root, "archive/project-plan-copy.md");
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
    }

    #[test]
    fn duplicate_group_is_not_reported_when_files_diverge_before_rescan() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        fs::write(folder.path().join("archive/project-plan-copy.md"), "changed outside Folio").unwrap();
        assert!(duplicate_groups(&conn, &root).unwrap().is_empty());
    }

    #[test]
    fn index_persists_across_reopen() {
        let folder = tempfile::tempdir().unwrap();
        copy_fixtures(folder.path());
        let data = tempfile::tempdir().unwrap();
        let database = data.path().join("folio.sqlite");
        let workspace_id = {
            let mut conn = db::open(&database).unwrap();
            let root = remember_picked_folder(&conn, folder.path()).unwrap();
            scan(&mut conn, &root);
            root.id
        };
        let mut conn = db::open(&database).unwrap();
        let root = workspace::reopen(&conn, &workspace_id).unwrap();
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
        assert_ne!(old, new);
        assert_eq!(register_space(&conn, &space("r1")).unwrap(), old);
        let pending = pending_embedding_chunks(&conn, &root.id, &old, 2).unwrap();
        put_embeddings(&mut conn, &root.id, &old, &[ChunkVector { chunk_id: pending[0].chunk_id, vector: vec![1.0, 0.0, 0.0] }]).unwrap();
        put_embeddings(&mut conn, &root.id, &new, &[ChunkVector { chunk_id: pending[1].chunk_id, vector: vec![1.0, 0.0, 0.0] }]).unwrap();
        let from_old = vector_candidates(&conn, &root.id, &old, &[1.0, 0.0, 0.0], 10).unwrap();
        assert_eq!(from_old.iter().map(|candidate| candidate.chunk_id).collect::<Vec<_>>(), vec![pending[0].chunk_id]);
        assert_eq!(pending_embedding_chunks(&conn, &root.id, &new, 1000).unwrap().iter().filter(|chunk| chunk.chunk_id == pending[0].chunk_id).count(), 1);
        let mismatch = put_embeddings(&mut conn, &root.id, &old, &[ChunkVector { chunk_id: pending[0].chunk_id, vector: vec![1.0; 4] }]).unwrap_err();
        assert_eq!(mismatch.code, ErrorCode::EmbeddingSpaceMismatch);
        assert_eq!(vector_candidates(&conn, &root.id, &old, &[1.0; 4], 3).unwrap_err().code, ErrorCode::EmbeddingSpaceMismatch);
    }

    #[test]
    fn edits_invalidate_embeddings_for_changed_chunks() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let space = register_space(&conn, &EmbeddingSpace { model_id: "m".into(), revision: "1".into(), quantization: "q".into(), dimensions: 2, preprocessing_fingerprint: "p".into() }).unwrap();
        let plan_id = id_of(&conn, &root, "projects/project-plan.md");
        let chunk_id: i64 = conn.query_row("SELECT chunk_id FROM chunks WHERE document_id = ?1", [&plan_id], |row| row.get(0)).unwrap();
        put_embeddings(&mut conn, &root.id, &space, &[ChunkVector { chunk_id, vector: vec![0.5, 0.5] }]).unwrap();
        fs::write(folder.path().join("projects/project-plan.md"), "# Plan\n\nRewritten content with a different length.").unwrap();
        scan(&mut conn, &root);
        let vectors: i64 = conn.query_row("SELECT count(*) FROM embeddings", [], |row| row.get(0)).unwrap();
        assert_eq!(vectors, 0);
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
