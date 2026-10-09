use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use crate::error::{fail, ErrorCode, NativeResult};
use crate::extract::{self, MediaKind};

/// One authorized folder. A Folio workspace is currently one active folder at a time (ADR 0005).
#[derive(Clone, Debug)]
pub struct ScopedRoot {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub root_path: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KnownWorkspace {
    pub id: String,
    pub root_path: String,
    pub authorized_at: String,
    pub last_opened_at: Option<String>,
    pub available: bool,
}

impl ScopedRoot {
    pub fn info(&self) -> WorkspaceInfo {
        WorkspaceInfo { id: self.id.clone(), root_path: self.path.to_string_lossy().into_owned() }
    }
}

pub fn now_millis() -> String {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| elapsed.as_millis()).unwrap_or(0).to_string()
}

/// The canonical root, or NOT_AUTHORIZED when the folder is gone or no longer readable.
pub fn available_root(path: &Path) -> NativeResult<PathBuf> {
    let lost = || fail(ErrorCode::NotAuthorized, "The authorized folder is unavailable or Folio lost permission. Choose the folder again.");
    let root = path.canonicalize().map_err(|_| lost())?;
    if !root.is_dir() || fs::read_dir(&root).is_err() {
        return Err(lost());
    }
    Ok(root)
}

/// Records a folder the user picked in the native dialog. Picking the same folder again
/// reuses its workspace id, so its index and history carry over.
pub fn remember_picked_folder(conn: &Connection, picked: &Path) -> NativeResult<ScopedRoot> {
    let path = available_root(picked)?;
    let root_path = path.to_string_lossy().into_owned();
    let now = now_millis();
    let existing: Option<String> = conn.query_row("SELECT id FROM workspaces WHERE root_path = ?1", [&root_path], |row| row.get(0)).optional()?;
    let id = match existing {
        Some(id) => {
            conn.execute("UPDATE workspaces SET last_opened_at = ?1 WHERE id = ?2", params![now, id])?;
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute("INSERT INTO workspaces (id, root_path, authorized_at, last_opened_at) VALUES (?1, ?2, ?3, ?3)", params![id, root_path, now])?;
            id
        }
    };
    Ok(ScopedRoot { id, path })
}

/// Re-authorizes a folder previously chosen through the native picker. The webview can only
/// name a stored workspace id, never a path.
pub fn reopen(conn: &Connection, workspace_id: &str) -> NativeResult<ScopedRoot> {
    let stored: String = conn
        .query_row("SELECT root_path FROM workspaces WHERE id = ?1", [workspace_id], |row| row.get(0))
        .optional()?
        .ok_or_else(|| fail(ErrorCode::NotAuthorized, "This folder was never authorized. Choose it with the folder picker."))?;
    let path = available_root(Path::new(&stored))?;
    if path != Path::new(&stored) {
        return Err(fail(ErrorCode::NotAuthorized, "The authorized folder now resolves to a different location. Choose it again."));
    }
    conn.execute("UPDATE workspaces SET last_opened_at = ?1 WHERE id = ?2", params![now_millis(), workspace_id])?;
    Ok(ScopedRoot { id: workspace_id.to_owned(), path })
}

pub fn known_workspaces(conn: &Connection) -> NativeResult<Vec<KnownWorkspace>> {
    let mut statement = conn.prepare("SELECT id, root_path, authorized_at, last_opened_at FROM workspaces ORDER BY last_opened_at DESC")?;
    let rows = statement.query_map([], |row| {
        let root_path: String = row.get(1)?;
        Ok(KnownWorkspace { id: row.get(0)?, available: available_root(Path::new(&root_path)).is_ok(), root_path, authorized_at: row.get(2)?, last_opened_at: row.get(3)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Resolves an existing document inside the root, refusing absolute paths, `..` escapes and
/// symlinks that point outside.
pub fn resolve_document(root: &Path, relative: &str) -> NativeResult<PathBuf> {
    if relative.is_empty() || Path::new(relative).is_absolute() || relative.starts_with(['/', '\\']) {
        return Err(fail(ErrorCode::PathEscape, "A relative document path is required."));
    }
    let root = available_root(root)?;
    let resolved = root.join(relative).canonicalize().map_err(|_| fail(ErrorCode::NotFound, "The document is unavailable."))?;
    if !resolved.starts_with(&root) {
        return Err(fail(ErrorCode::PathEscape, "The document is outside the authorized folder."));
    }
    if !resolved.is_file() {
        return Err(fail(ErrorCode::NotFound, "The document is unavailable."));
    }
    Ok(resolved)
}

/// Reads at most `limit` bytes; larger files are refused rather than truncated.
pub fn read_bounded(path: &Path, limit: u64) -> NativeResult<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(fail(ErrorCode::TooLarge, format!("This file is larger than the {} MiB limit.", limit / 1024 / 1024)));
    }
    Ok(bytes)
}

/// The document's extracted text: TXT/Markdown as written, text PDFs page by page.
pub fn read_text(root: &Path, relative: &str) -> NativeResult<String> {
    let path = resolve_document(root, relative)?;
    let kind = MediaKind::from_path(&path).ok_or_else(|| fail(ErrorCode::Unsupported, "Folio reads TXT, Markdown and text-based PDF files."))?;
    extract::document_text(kind, &read_bounded(&path, kind.max_bytes())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn reads_authorized_filipino_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tala.md"), "Ang pagpupulong ay sa Biyernes.").unwrap();
        assert_eq!(read_text(root.path(), "tala.md").unwrap(), "Ang pagpupulong ay sa Biyernes.");
    }

    #[test]
    fn rejects_parent_escape_and_absolute_paths() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        fs::write(parent.path().join("outside.md"), "private").unwrap();
        assert_eq!(resolve_document(&root, "../outside.md").unwrap_err().code, ErrorCode::PathEscape);
        assert_eq!(resolve_document(&root, parent.path().join("outside.md").to_str().unwrap()).unwrap_err().code, ErrorCode::PathEscape);
    }

    #[test]
    fn reads_text_pdf_and_refuses_scanned_pdf() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("guide.pdf"), extract::testpdf::text_pdf(&[&["Consent form guide."]])).unwrap();
        fs::write(root.path().join("scan.pdf"), extract::testpdf::image_only_pdf()).unwrap();
        assert!(read_text(root.path(), "guide.pdf").unwrap().contains("Consent form guide."));
        assert_eq!(read_text(root.path(), "scan.pdf").unwrap_err().code, ErrorCode::Unsupported);
    }

    #[test]
    fn rejects_symlink_escape() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        let outside = parent.path().join("outside.md");
        fs::write(&outside, "private").unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("link.md"));
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&outside, root.join("link.md"));
        if linked.is_err() {
            eprintln!("skipping: this host does not permit creating symlinks");
            return;
        }
        assert_eq!(read_text(&root, "link.md").unwrap_err().code, ErrorCode::PathEscape);
    }

    #[test]
    fn picked_folder_is_remembered_and_reopened_with_the_same_id() {
        let conn = db::open_in_memory().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let first = remember_picked_folder(&conn, folder.path()).unwrap();
        let again = remember_picked_folder(&conn, folder.path()).unwrap();
        assert_eq!(first.id, again.id);
        assert_eq!(reopen(&conn, &first.id).unwrap().path, first.path);
        assert_eq!(reopen(&conn, "never-picked").unwrap_err().code, ErrorCode::NotAuthorized);
    }

    #[test]
    fn reopening_a_removed_folder_is_refused() {
        let conn = db::open_in_memory().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let root = remember_picked_folder(&conn, folder.path()).unwrap();
        drop(folder);
        assert_eq!(reopen(&conn, &root.id).unwrap_err().code, ErrorCode::NotAuthorized);
        assert!(!known_workspaces(&conn).unwrap()[0].available);
    }
}
