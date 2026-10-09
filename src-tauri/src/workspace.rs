use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use walkdir::WalkDir;

use crate::db::NativeResult;
use crate::error::{error, ErrorCode, FolioError};
use crate::extract::{self, MediaKind};
use crate::identity::{
    content_hash, document_id, media_type_for_path, normalize_relative_path, relative_path_below,
    workspace_id_for,
};

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 5000;

#[derive(Clone, Debug)]
pub struct ScopedRoot {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub root_path: String,
    pub authorized_at: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub id: String,
    pub workspace_id: String,
    pub relative_path: String,
    pub name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub modified_at_ms: Option<u64>,
}

/// A file Folio found but will not present as a document, with the reason.
/// Dropping it silently would misreport what the folder contains.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedEntry {
    pub display_name: String,
    pub code: ErrorCode,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentListing {
    pub workspace_id: String,
    pub documents: Vec<DocumentMetadata>,
    pub skipped: Vec<SkippedEntry>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentText {
    pub content: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub modified_at_ms: Option<u64>,
}

/// The authorized folders of this session. A command reaches the filesystem
/// only through a root this registry issued, so a workspace identity the UI
/// made up resolves to nothing.
#[derive(Default)]
pub struct WorkspaceRegistry {
    roots: BTreeMap<String, ScopedRoot>,
}

fn epoch_ms(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|value| value.as_millis() as u64)
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Authorize a folder the user picked. The identity is derived from the
    /// canonical path, so choosing the same folder again — including after a
    /// restart — returns the same workspace.
    pub fn authorize(&mut self, path: &Path) -> Result<WorkspaceInfo, FolioError> {
        let canonical = path.canonicalize().map_err(|_| {
            error(
                ErrorCode::WorkspaceUnavailable,
                "That folder is no longer available. Choose it again to continue.",
            )
        })?;
        if !canonical.is_dir() {
            return Err(error(
                ErrorCode::WorkspaceUnavailable,
                "Choose a folder, not a file.",
            ));
        }
        let id = workspace_id_for(&canonical)?;
        let info = WorkspaceInfo {
            id: id.clone(),
            root_path: canonical.to_string_lossy().into_owned(),
            authorized_at: epoch_ms(SystemTime::now()).unwrap_or_default(),
        };
        self.roots.insert(
            id.clone(),
            ScopedRoot {
                id,
                path: canonical,
            },
        );
        Ok(info)
    }

    /// The workspace gate. Every filesystem command starts here.
    pub fn resolve(&self, workspace_id: &str) -> Result<ScopedRoot, FolioError> {
        let root = self.roots.get(workspace_id).cloned().ok_or_else(|| {
            error(
                ErrorCode::WorkspaceNotAuthorized,
                "Select an authorized folder first.",
            )
        })?;
        if !root.path.is_dir() {
            return Err(error(
                ErrorCode::WorkspaceUnavailable,
                "That folder is no longer available. Choose it again to continue.",
            ));
        }
        Ok(root)
    }
}

/// Resolve a relative path inside an authorized folder, refusing anything that
/// escapes it — including through a symbolic link.
pub fn resolve_document(root: &Path, relative: &str) -> Result<PathBuf, FolioError> {
    let relative = normalize_relative_path(relative)?;
    let root = root.canonicalize().map_err(|_| {
        error(
            ErrorCode::WorkspaceUnavailable,
            "That folder is no longer available. Choose it again to continue.",
        )
    })?;
    let resolved = root.join(&relative).canonicalize().map_err(|_| {
        error(
            ErrorCode::DocumentUnavailable,
            "That document is unavailable.",
        )
        .with_detail("path", relative.as_str())
    })?;
    if !resolved.starts_with(&root) {
        return Err(error(
            ErrorCode::PathEscapesWorkspace,
            "That document is outside the authorized folder.",
        )
        .with_detail("path", relative.as_str()));
    }
    if !resolved.is_file() {
        return Err(error(
            ErrorCode::DocumentUnavailable,
            "That document is unavailable.",
        )
        .with_detail("path", relative.as_str()));
    }
    Ok(resolved)
}

/// List the documents in an authorized folder. Symbolic links are not followed,
/// and nothing outside the folder is reported.
pub fn list_documents(root: &ScopedRoot) -> Result<DocumentListing, FolioError> {
    let canonical = root.path.canonicalize().map_err(|_| {
        error(
            ErrorCode::WorkspaceUnavailable,
            "That folder is no longer available. Choose it again to continue.",
        )
    })?;
    let mut documents = Vec::new();
    let mut skipped = Vec::new();
    for entry in WalkDir::new(&canonical).follow_links(false) {
        let entry =
            entry.map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = match relative_path_below(&canonical, entry.path()) {
            Ok(value) => value,
            Err(failure) => {
                skipped.push(SkippedEntry {
                    display_name: entry.file_name().to_string_lossy().into_owned(),
                    code: failure.code,
                });
                continue;
            }
        };
        let Some(media_type) = media_type_for_path(&relative_path) else {
            continue;
        };
        let metadata = entry
            .metadata()
            .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
        documents.push(DocumentMetadata {
            id: document_id(&root.id, &relative_path),
            workspace_id: root.id.clone(),
            name: relative_path
                .rsplit('/')
                .next()
                .unwrap_or(&relative_path)
                .to_string(),
            relative_path,
            media_type: media_type.to_string(),
            size_bytes: metadata.len(),
            modified_at_ms: metadata.modified().ok().and_then(epoch_ms),
        });
        if documents.len() > MAX_DOCUMENTS {
            return Err(error(
                ErrorCode::DocumentTooLarge,
                "This starter supports up to 5,000 documents per selected folder. Choose a smaller folder.",
            ));
        }
    }
    documents.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    skipped.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(DocumentListing {
        workspace_id: root.id.clone(),
        documents,
        skipped,
    })
}

/// Read a TXT or Markdown document together with the revision it was read at.
pub fn read_text(root: &Path, relative: &str) -> Result<DocumentText, FolioError> {
    let path = resolve_document(root, relative)?;
    let media_type = media_type_for_path(relative).ok_or_else(|| {
        error(
            ErrorCode::UnsupportedMediaType,
            "Folio reads TXT, Markdown and text-based PDF documents.",
        )
    })?;
    let metadata = fs::metadata(&path)
        .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
    if media_type == "application/pdf" {
        // Text-based PDFs are read/index-only. `content` is the extracted text
        // (pages joined by a blank line); the hash and size are of the file bytes.
        let bytes = read_bounded(&path, extract::MAX_PDF_BYTES)?;
        let content = extract::document_text(MediaKind::Pdf, &bytes)
            .map_err(|failure| failure.with_detail("path", relative))?;
        return Ok(DocumentText {
            content_hash: content_hash(&bytes),
            size_bytes: bytes.len() as u64,
            modified_at_ms: metadata.modified().ok().and_then(epoch_ms),
            content,
        });
    }
    let file = fs::File::open(&path)
        .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
    if bytes.len() as u64 > MAX_TEXT_BYTES {
        return Err(error(
            ErrorCode::DocumentTooLarge,
            "This starter reads text files up to 2 MiB.",
        )
        .with_detail("path", relative));
    }
    let content = String::from_utf8(bytes).map_err(|_| {
        error(
            ErrorCode::DocumentNotText,
            "This document is not valid UTF-8 text.",
        )
        .with_detail("path", relative)
    })?;
    Ok(DocumentText {
        content_hash: content_hash(content.as_bytes()),
        size_bytes: content.as_bytes().len() as u64,
        modified_at_ms: metadata.modified().ok().and_then(epoch_ms),
        content,
    })
}

/// The current hash of a document, used by plan preflight without loading the
/// whole corpus.
pub fn document_hash(root: &Path, relative: &str) -> Result<String, FolioError> {
    let path = resolve_document(root, relative)?;
    let bytes =
        fs::read(path).map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
    Ok(content_hash(&bytes))
}

/// The canonical root, or `workspaceUnavailable` when the folder is gone or unreadable.
pub fn available_root(path: &Path) -> Result<PathBuf, FolioError> {
    let unavailable = || {
        error(
            ErrorCode::WorkspaceUnavailable,
            "That folder is no longer available. Choose it again to continue.",
        )
    };
    let root = path.canonicalize().map_err(|_| unavailable())?;
    if !root.is_dir() || fs::read_dir(&root).is_err() {
        return Err(unavailable());
    }
    Ok(root)
}

/// Reads at most `limit` bytes; a larger file is refused rather than truncated.
pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, FolioError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
    if bytes.len() as u64 > limit {
        return Err(error(
            ErrorCode::DocumentTooLarge,
            format!("This file is larger than the {} MiB limit.", limit / 1024 / 1024),
        ));
    }
    Ok(bytes)
}

/// A folder the user picked in an earlier session (ADR 0007).
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnownWorkspace {
    pub id: String,
    pub root_path: String,
    pub authorized_at: u64,
    pub last_opened_at: Option<u64>,
    /// False when the folder is gone or no longer readable.
    pub available: bool,
}

/// Records a folder authorized through the native picker so a later session
/// can restore it. The webview can only ever name a stored identity.
pub fn remember(conn: &Connection, info: &WorkspaceInfo) -> NativeResult<()> {
    let now = epoch_ms(SystemTime::now()).unwrap_or_default().to_string();
    conn.execute(
        "INSERT INTO workspaces (id, root_path, authorized_at, last_opened_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path, last_opened_at = excluded.last_opened_at",
        params![info.id, info.root_path, info.authorized_at.to_string(), now],
    )?;
    Ok(())
}

/// The stored root of a previously picked folder. The caller re-authorizes it
/// through `WorkspaceRegistry::authorize`, which revalidates access.
pub fn remembered_root(conn: &Connection, workspace_id: &str) -> NativeResult<PathBuf> {
    conn.query_row(
        "SELECT root_path FROM workspaces WHERE id = ?1",
        [workspace_id],
        |row| row.get::<_, String>(0),
    )
    .optional()?
    .map(PathBuf::from)
    .ok_or_else(|| {
        error(
            ErrorCode::WorkspaceNotAuthorized,
            "This folder was never chosen in Folio. Choose it with the folder picker.",
        )
    })
}

/// The remembered folders as stored, without touching the filesystem. `available` is
/// filled in by `with_availability`, which may block on a slow or unreachable path and so
/// runs without holding the index.
pub fn remembered_workspaces(conn: &Connection) -> NativeResult<Vec<KnownWorkspace>> {
    let mut statement = conn.prepare(
        "SELECT id, root_path, authorized_at, last_opened_at FROM workspaces
         ORDER BY CAST(last_opened_at AS INTEGER) DESC",
    )?;
    let rows = statement.query_map([], |row| {
        let authorized_at: String = row.get(2)?;
        let last_opened_at: Option<String> = row.get(3)?;
        Ok(KnownWorkspace {
            id: row.get(0)?,
            root_path: row.get(1)?,
            authorized_at: authorized_at.parse().unwrap_or_default(),
            last_opened_at: last_opened_at.and_then(|value| value.parse().ok()),
            available: false,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn with_availability(mut workspaces: Vec<KnownWorkspace>) -> Vec<KnownWorkspace> {
    for workspace in &mut workspaces {
        workspace.available = available_root(Path::new(&workspace.root_path)).is_ok();
    }
    workspaces
}

#[cfg(test)]
pub fn known_workspaces(conn: &Connection) -> NativeResult<Vec<KnownWorkspace>> {
    Ok(with_availability(remembered_workspaces(conn)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(root: &Path) -> (WorkspaceRegistry, WorkspaceInfo) {
        let mut registry = WorkspaceRegistry::new();
        let info = registry.authorize(root).unwrap();
        (registry, info)
    }

    #[test]
    fn reads_authorized_filipino_text_with_its_revision() {
        let root = tempfile::tempdir().unwrap();
        let text = "Ang pagpupulong ay sa Biyernes.";
        fs::write(root.path().join("tala.md"), text).unwrap();
        let read = read_text(root.path(), "tala.md").unwrap();
        assert_eq!(read.content, text);
        assert_eq!(read.content_hash, content_hash(text.as_bytes()));
        assert_eq!(read.size_bytes, text.as_bytes().len() as u64);
    }

    #[test]
    fn reports_byte_length_not_character_count() {
        let root = tempfile::tempdir().unwrap();
        // 'ñ' and the emoji are multi-byte: a character count would be wrong.
        let text = "Ni\u{00f1}a \u{1f4c5}";
        fs::write(root.path().join("tala.md"), text).unwrap();
        let read = read_text(root.path(), "tala.md").unwrap();
        assert_eq!(read.size_bytes, 10);
        assert_eq!(read.content.chars().count(), 6);
    }

    #[test]
    fn refuses_parent_escape_and_absolute_paths() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        fs::write(parent.path().join("outside.md"), "private").unwrap();
        assert_eq!(
            resolve_document(&root, "../outside.md").unwrap_err().code,
            ErrorCode::PathEscapesWorkspace
        );
        let absolute = parent.path().join("outside.md");
        assert_eq!(
            resolve_document(&root, absolute.to_str().unwrap())
                .unwrap_err()
                .code,
            ErrorCode::PathNotRelative
        );
        assert_eq!(fs::read_to_string(&absolute).unwrap(), "private");
    }

    #[test]
    fn refuses_pdf_as_plain_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("paper.pdf"), "%PDF").unwrap();
        assert_eq!(
            read_text(root.path(), "paper.pdf").unwrap_err().code,
            ErrorCode::DocumentNotText
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_that_leaves_the_folder() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        let outside = parent.path().join("outside.md");
        fs::write(&outside, "private").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("link.md")).unwrap();
        assert_eq!(
            read_text(&root, "link.md").unwrap_err().code,
            ErrorCode::PathEscapesWorkspace
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "private");
    }

    #[cfg(unix)]
    #[test]
    fn never_lists_a_document_through_a_symlinked_folder() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        let outside = parent.path().join("private");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.md"), "private").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("shortcut")).unwrap();
        fs::write(root.join("tala.md"), "Nasa loob").unwrap();
        let (registry, info) = registry_with(&root);
        let listing = list_documents(&registry.resolve(&info.id).unwrap()).unwrap();
        let paths: Vec<_> = listing
            .documents
            .iter()
            .map(|d| d.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["tala.md"]);
    }

    #[test]
    fn lists_only_the_formats_folio_identifies() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tala.md"), "a").unwrap();
        fs::write(root.path().join("notes.txt"), "b").unwrap();
        fs::write(root.path().join("paper.pdf"), "%PDF").unwrap();
        fs::write(root.path().join("sheet.xlsx"), "c").unwrap();
        let (registry, info) = registry_with(root.path());
        let listing = list_documents(&registry.resolve(&info.id).unwrap()).unwrap();
        let paths: Vec<_> = listing
            .documents
            .iter()
            .map(|d| d.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["notes.txt", "paper.pdf", "tala.md"]);
        assert!(listing.skipped.is_empty());
    }

    // macOS rejects a filename that is not valid UTF-8 at creation time
    // ("Illegal byte sequence"), and Windows filenames are UTF-16, so only a
    // host that stores arbitrary bytes can put Folio in this situation. The
    // refusal itself is covered everywhere by
    // `identity::tests::refuses_a_filename_that_is_not_valid_unicode`.
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn reports_a_file_it_cannot_identify_instead_of_dropping_it() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let root = tempfile::tempdir().unwrap();
        // An invalid UTF-8 filename: lossy conversion would invent a name.
        let name = OsStr::from_bytes(b"tala-\xff.md");
        fs::write(root.path().join(name), "a").unwrap();
        fs::write(root.path().join("ok.md"), "b").unwrap();
        let (registry, info) = registry_with(root.path());
        let listing = list_documents(&registry.resolve(&info.id).unwrap()).unwrap();
        assert_eq!(listing.documents.len(), 1);
        assert_eq!(listing.skipped.len(), 1);
        assert_eq!(listing.skipped[0].code, ErrorCode::PathUnsupportedEncoding);
    }

    #[test]
    fn gives_every_document_a_workspace_scoped_identity() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("notes")).unwrap();
        fs::write(root.path().join("notes").join("paalala.md"), "a").unwrap();
        let (registry, info) = registry_with(root.path());
        let listing = list_documents(&registry.resolve(&info.id).unwrap()).unwrap();
        let document = &listing.documents[0];
        assert_eq!(document.workspace_id, info.id);
        assert_eq!(document.id, format!("{}:notes/paalala.md", info.id));
        assert_ne!(document.id, document.relative_path);
    }

    #[test]
    fn refuses_a_workspace_identity_it_never_issued() {
        let root = tempfile::tempdir().unwrap();
        let (registry, info) = registry_with(root.path());
        assert_eq!(
            registry.resolve("made-up-by-the-ui").unwrap_err().code,
            ErrorCode::WorkspaceNotAuthorized
        );
        assert!(registry.resolve(&info.id).is_ok());
    }

    #[test]
    fn keeps_a_workspace_identity_across_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let mut first = WorkspaceRegistry::new();
        let before = first.authorize(root.path()).unwrap();
        let mut after_restart = WorkspaceRegistry::new();
        assert_eq!(
            after_restart.resolve(&before.id).unwrap_err().code,
            ErrorCode::WorkspaceNotAuthorized
        );
        let after = after_restart.authorize(root.path()).unwrap();
        assert_eq!(after.id, before.id);
    }

    #[test]
    fn refuses_a_folder_that_disappeared() {
        let root = tempfile::tempdir().unwrap();
        let (mut registry, _) = registry_with(root.path());
        let nested = root.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let info = registry.authorize(&nested).unwrap();
        fs::remove_dir(&nested).unwrap();
        assert_eq!(
            registry.resolve(&info.id).unwrap_err().code,
            ErrorCode::WorkspaceUnavailable
        );
    }

    #[test]
    fn reads_a_text_pdf_and_refuses_a_scanned_one() {
        let root = tempfile::tempdir().unwrap();
        let pdf = extract::testpdf::text_pdf(&[&["Consent form guide."]]);
        fs::write(root.path().join("guide.pdf"), &pdf).unwrap();
        fs::write(root.path().join("scan.pdf"), extract::testpdf::image_only_pdf()).unwrap();
        let read = read_text(root.path(), "guide.pdf").unwrap();
        assert!(read.content.contains("Consent form guide."));
        assert_eq!(read.content_hash, content_hash(&pdf));
        assert_eq!(read.size_bytes, pdf.len() as u64);
        assert_eq!(
            read_text(root.path(), "scan.pdf").unwrap_err().code,
            ErrorCode::DocumentNotText
        );
    }

    #[test]
    fn a_remembered_folder_is_restored_with_the_same_identity() {
        let conn = crate::db::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (_, info) = registry_with(root.path());
        remember(&conn, &info).unwrap();
        let mut after_restart = WorkspaceRegistry::new();
        let restored = after_restart
            .authorize(&remembered_root(&conn, &info.id).unwrap())
            .unwrap();
        assert_eq!(restored.id, info.id);
        assert_eq!(
            remembered_root(&conn, "never-picked").unwrap_err().code,
            ErrorCode::WorkspaceNotAuthorized
        );
        assert!(known_workspaces(&conn).unwrap()[0].available);
    }

    #[test]
    fn a_remembered_folder_that_disappeared_is_reported_unavailable() {
        let conn = crate::db::open_in_memory().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (_, info) = registry_with(root.path());
        remember(&conn, &info).unwrap();
        drop(root);
        assert!(!known_workspaces(&conn).unwrap()[0].available);
        let mut registry = WorkspaceRegistry::new();
        assert_eq!(
            registry
                .authorize(&remembered_root(&conn, &info.id).unwrap())
                .unwrap_err()
                .code,
            ErrorCode::WorkspaceUnavailable
        );
    }

    #[test]
    fn refuses_a_symlink_escape_on_hosts_that_can_create_one() {
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
        assert_eq!(
            read_text(&root, "link.md").unwrap_err().code,
            ErrorCode::PathEscapesWorkspace
        );
    }
}
