use std::path::{Component, Path};

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::error::{error, ErrorCode, FolioError};

/// Characters Windows refuses inside a path segment, plus the separator Folio
/// never accepts inside a segment.
const NOT_PORTABLE: [char; 8] = ['<', '>', ':', '"', '|', '?', '*', '\\'];

fn is_control(value: char) -> bool {
    value <= '\u{1f}' || value == '\u{7f}'
}

/// `sha256:<64 lowercase hex>` over exact bytes.
pub fn content_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", hex(bytes))
}

fn hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Validate and canonicalize a workspace-relative path.
///
/// NFC normalization keeps a Filipino filename identical whether macOS reports
/// it decomposed or Windows reports it composed. Anything ambiguous is refused
/// rather than repaired, because a repaired path identifies a different file.
pub fn normalize_relative_path(raw: &str) -> Result<String, FolioError> {
    if raw.is_empty() {
        return Err(error(
            ErrorCode::PathNotRelative,
            "A relative document path is required.",
        ));
    }
    let value: String = raw.nfc().collect();
    if value.chars().any(is_control) {
        return Err(error(
            ErrorCode::PathNotRelative,
            "A document path cannot contain control characters.",
        ));
    }
    if value.contains('\\') {
        return Err(error(
            ErrorCode::PathNotRelative,
            "Use '/' to separate path segments.",
        )
        .with_detail("path", raw));
    }
    let mut characters = value.chars();
    let first = characters.next();
    if first == Some('/')
        || (first.map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
            && characters.next() == Some(':'))
    {
        return Err(error(
            ErrorCode::PathNotRelative,
            "An absolute path cannot identify a document inside an authorized folder.",
        )
        .with_detail("path", raw));
    }
    for segment in value.split('/') {
        if segment.is_empty() {
            return Err(error(
                ErrorCode::PathNotRelative,
                "A document path cannot contain an empty segment.",
            )
            .with_detail("path", raw));
        }
        if segment == "." || segment == ".." {
            return Err(error(
                ErrorCode::PathEscapesWorkspace,
                "A document path cannot navigate outside the authorized folder.",
            )
            .with_detail("path", raw));
        }
    }
    Ok(value)
}

/// Extra check for a path Folio would create, so a plan previewed on macOS does
/// not fail halfway through on Windows.
pub fn assert_portable_destination(raw: &str) -> Result<String, FolioError> {
    let path = normalize_relative_path(raw)?;
    for segment in path.split('/') {
        let unportable = segment.chars().any(|c| NOT_PORTABLE.contains(&c))
            || segment.ends_with(' ')
            || segment.ends_with('.');
        if unportable {
            return Err(error(
                ErrorCode::OperationUnsupported,
                format!(
                    "'{segment}' cannot be stored on every supported platform. Choose another name."
                ),
            )
            .with_detail("path", path.as_str()));
        }
    }
    Ok(path)
}

/// Build the relative path of `entry` below `root` from path components, so a
/// filename containing a backslash on Unix is never rewritten into two
/// segments, and a non-Unicode name is refused instead of being replaced with
/// U+FFFD.
pub fn relative_path_below(root: &Path, entry: &Path) -> Result<String, FolioError> {
    let suffix = entry.strip_prefix(root).map_err(|_| {
        error(
            ErrorCode::PathEscapesWorkspace,
            "That document is outside the authorized folder.",
        )
    })?;
    let mut segments = Vec::new();
    for component in suffix.components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_str().ok_or_else(|| {
                    error(
                        ErrorCode::PathUnsupportedEncoding,
                        "This file's name is not valid Unicode, so Folio cannot identify it reliably.",
                    )
                })?;
                segments.push(text.nfc().collect::<String>());
            }
            Component::CurDir => continue,
            _ => {
                return Err(error(
                    ErrorCode::PathEscapesWorkspace,
                    "That document is outside the authorized folder.",
                ))
            }
        }
    }
    if segments.is_empty() {
        return Err(error(
            ErrorCode::PathNotRelative,
            "A relative document path is required.",
        ));
    }
    let joined = segments.join("/");
    normalize_relative_path(&joined)
}

/// Workspace identity derived from the canonical root path, so the same folder
/// keeps its identity across restarts and a restored preview still points at it.
pub fn workspace_id_for(canonical_root: &Path) -> Result<String, FolioError> {
    let text = canonical_root.to_str().ok_or_else(|| {
        error(
            ErrorCode::PathUnsupportedEncoding,
            "That folder's path is not valid Unicode, so Folio cannot identify it reliably.",
        )
    })?;
    let normalized: String = text.nfc().collect();
    Ok(hex(normalized.as_bytes()))
}

/// `${workspaceId}:${relativePath}` — reversible and never lossy.
pub fn document_id(workspace_id: &str, relative_path: &str) -> String {
    format!("{workspace_id}:{relative_path}")
}

/// The media types Folio identifies; anything else is not a Folio document.
pub fn media_type_for_path(relative_path: &str) -> Option<&'static str> {
    let name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_lowercase())?;
    match extension.as_str() {
        "md" | "markdown" => Some("text/markdown"),
        "txt" => Some("text/plain"),
        "pdf" => Some("application/pdf"),
        _ => None,
    }
}

pub fn is_editable_media_type(media_type: &str) -> bool {
    matches!(media_type, "text/plain" | "text/markdown")
}

#[allow(dead_code)]
fn escape_field(value: &str) -> String {
    value.replace('%', "%25").replace('/', "%2F")
}

/// Canonical single-string identity of a vector space. Two indexes may only be
/// compared when their fingerprints are equal.
#[allow(dead_code)]
pub fn embedding_space_fingerprint(
    model_id: &str,
    revision: &str,
    quantization: &str,
    dimensions: u32,
    preprocessing_fingerprint: &str,
) -> String {
    format!(
        "folio-space-v1/{}/{}/{}/{}/{}",
        escape_field(model_id),
        escape_field(revision),
        escape_field(quantization),
        dimensions,
        escape_field(preprocessing_fingerprint)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn refuses_paths_that_reach_outside_the_folder() {
        assert_eq!(
            normalize_relative_path("../outside.md").unwrap_err().code,
            ErrorCode::PathEscapesWorkspace
        );
        assert_eq!(
            normalize_relative_path("/etc/passwd").unwrap_err().code,
            ErrorCode::PathNotRelative
        );
        assert_eq!(
            normalize_relative_path("C:/Users/x/notes.md")
                .unwrap_err()
                .code,
            ErrorCode::PathNotRelative
        );
    }

    #[test]
    fn gives_a_decomposed_filipino_filename_one_identity() {
        let decomposed = "courses/pagsasanay-n\u{0303}.md";
        let composed = "courses/pagsasanay-\u{00f1}.md";
        assert_eq!(normalize_relative_path(decomposed).unwrap(), composed);
        assert_eq!(
            document_id("w", &normalize_relative_path(decomposed).unwrap()),
            document_id("w", composed)
        );
    }

    #[test]
    fn keeps_a_backslash_in_a_unix_filename_out_of_an_identity() {
        // The old implementation rewrote '\\' into '/', which silently renamed
        // a legitimate Unix file into a two-segment path.
        assert_eq!(
            normalize_relative_path("notes\\paalala.md")
                .unwrap_err()
                .code,
            ErrorCode::PathNotRelative
        );
        let root = tempfile::tempdir().unwrap();
        let name = "notes\\paalala.md";
        let file = root.path().join(name);
        fs::write(&file, "x").unwrap();
        assert_eq!(
            relative_path_below(root.path(), &file).unwrap_err().code,
            ErrorCode::PathNotRelative
        );
    }

    #[test]
    fn builds_a_relative_path_from_components() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("notes")).unwrap();
        let file = root.path().join("notes").join("tala sa proyekto.md");
        fs::write(&file, "x").unwrap();
        assert_eq!(
            relative_path_below(root.path(), &file).unwrap(),
            "notes/tala sa proyekto.md"
        );
    }

    #[test]
    fn derives_a_stable_workspace_identity_from_the_folder() {
        let root = tempfile::tempdir().unwrap();
        let first = workspace_id_for(root.path()).unwrap();
        let second = workspace_id_for(root.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(!first.contains(':'));
        let other = tempfile::tempdir().unwrap();
        assert_ne!(first, workspace_id_for(other.path()).unwrap());
    }

    #[test]
    fn refuses_destinations_that_windows_cannot_store() {
        assert_eq!(
            assert_portable_destination("notes/plano?.md")
                .unwrap_err()
                .code,
            ErrorCode::OperationUnsupported
        );
        assert_eq!(
            assert_portable_destination("notes/plano.md ")
                .unwrap_err()
                .code,
            ErrorCode::OperationUnsupported
        );
        assert_eq!(
            assert_portable_destination("notes/plano-2026.md").unwrap(),
            "notes/plano-2026.md"
        );
    }

    #[test]
    fn hashes_match_the_documented_form() {
        assert_eq!(
            content_hash(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
