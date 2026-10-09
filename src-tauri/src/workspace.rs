use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use walkdir::WalkDir;

use crate::error::{error, ErrorCode, FolioError};
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
    if media_type == "application/pdf" {
        return Err(error(
            ErrorCode::DocumentNotText,
            "PDF extraction is not connected yet. This reader supports TXT and Markdown.",
        )
        .with_detail("path", relative));
    }
    let metadata = fs::metadata(&path)
        .map_err(|cause| error(ErrorCode::DocumentUnavailable, cause.to_string()))?;
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

    #[cfg(unix)]
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
}
