use crate::error::FolioError;
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
}
