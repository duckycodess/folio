use crate::error::{error, ErrorCode, FolioError};
use crate::identity::normalize_relative_path;
use crate::workspace::ScopedRoot;

use super::{unavailable_state, Folio};

/// Resolve every provider request through the native workspace registry. The
/// provider layer receives only the scoped root issued by that registry.
pub(crate) fn resolve_workspace(
    state: &Folio,
    workspace_id: &str,
) -> Result<ScopedRoot, FolioError> {
    let workspaces = state.workspaces.lock().map_err(|_| unavailable_state())?;
    workspaces.resolve(workspace_id)
}

/// Split the #2 document identity without turning a caller-provided ID into a
/// filesystem path. The workspace prefix is checked before normalizing the
/// relative suffix, so a document from another authorized folder cannot be
/// used with this request.
pub(crate) fn parse_document_id(
    workspace_id: &str,
    document_id: &str,
) -> Result<String, FolioError> {
    let Some((prefix, relative_path)) = document_id.split_once(':') else {
        return Err(error(
            ErrorCode::PathNotRelative,
            "A #2 document ID is required.",
        ));
    };
    if prefix.is_empty() || relative_path.is_empty() {
        return Err(error(
            ErrorCode::PathNotRelative,
            "A #2 document ID must contain a workspace ID and relative path.",
        ));
    }
    if prefix != workspace_id {
        return Err(error(
            ErrorCode::WorkspaceNotAuthorized,
            "That document belongs to a different workspace.",
        )
        .with_detail("workspaceId", prefix));
    }
    normalize_relative_path(relative_path)
}

pub(crate) fn validate_document_filter(
    workspace_id: &str,
    document_id: Option<&str>,
) -> Result<Option<String>, FolioError> {
    document_id
        .map(|document_id| {
            parse_document_id(workspace_id, document_id).map(|_| document_id.to_owned())
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceRegistry;
    use std::fs;

    #[test]
    fn unknown_workspace_is_rejected_by_the_native_registry() {
        let state = Folio::new();
        let error = resolve_workspace(&state, "made-up-workspace").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::WorkspaceNotAuthorized);
    }

    #[test]
    fn authorized_workspace_resolves_to_the_registry_root() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes.md"), "notes").unwrap();
        let mut registry = WorkspaceRegistry::new();
        let info = registry.authorize(root.path()).unwrap();
        let state = Folio {
            workspaces: std::sync::Mutex::new(registry),
            plans: std::sync::Mutex::new(crate::plan::PlanRegistry::new()),
        };

        let resolved = resolve_workspace(&state, &info.id).unwrap();
        assert_eq!(resolved.id, info.id);
        assert_eq!(resolved.path, root.path().canonicalize().unwrap());
    }

    #[test]
    fn document_id_parser_returns_the_normalized_relative_path() {
        assert_eq!(
            parse_document_id("workspace", "workspace:notes/paalala.md").unwrap(),
            "notes/paalala.md"
        );
    }

    #[test]
    fn document_id_parser_rejects_foreign_and_malformed_ids() {
        assert_eq!(
            parse_document_id("workspace", "other:notes.md")
                .unwrap_err()
                .code,
            crate::error::ErrorCode::WorkspaceNotAuthorized
        );
        for input in [
            "../notes.md",
            "/tmp/notes.md",
            r"notes\paalala.md",
            "notes.md",
        ] {
            assert_eq!(
                parse_document_id("workspace", input).unwrap_err().code,
                crate::error::ErrorCode::PathNotRelative,
                "input {input:?}"
            );
        }
        assert_eq!(
            parse_document_id("workspace", "workspace:../notes.md")
                .unwrap_err()
                .code,
            crate::error::ErrorCode::PathEscapesWorkspace
        );
    }

    #[test]
    fn answer_document_filter_rejects_a_bare_path_before_search() {
        assert_eq!(
            validate_document_filter("workspace", Some("notes.md"))
                .unwrap_err()
                .code,
            crate::error::ErrorCode::PathNotRelative
        );
    }
}
