use crate::error::{error, ErrorCode, FolioError};
use crate::identity::normalize_relative_path;
use crate::workspace::ScopedRoot;
use folio_core::contracts::{NativeProviderError, ProviderErrorCode};
use folio_core::error::{CoreError, NativeProviderErrorError};
use sha2::{Digest, Sha256};

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

/// Convert the core provider vocabulary to the frozen #2 wire vocabulary.
/// Core errors never cross the Tauri boundary directly.
pub(crate) fn provider_failure(failure: NativeProviderError) -> FolioError {
    let NativeProviderError {
        code,
        message,
        detail,
    } = failure;
    match code {
        ProviderErrorCode::ModelNotInstalled => {
            let mut result = error(ErrorCode::ModelNotInstalled, message);
            if let Some(model_id) = detail {
                result = result.with_detail("modelId", model_id);
            }
            result
        }
        ProviderErrorCode::RuntimeMissing => {
            let mut result =
                error(ErrorCode::ModelNotInstalled, message).with_detail("component", "runtime");
            if let Some(runtime_id) = detail {
                result = result.with_detail("runtimeId", runtime_id);
            }
            result
        }
        ProviderErrorCode::ModelCorrupt => {
            error(ErrorCode::ModelLoadFailed, message).with_detail("reason", "verificationFailed")
        }
        ProviderErrorCode::RuntimeStartFailed => {
            error(ErrorCode::ModelLoadFailed, message).with_detail("reason", "runtimeStartFailed")
        }
        ProviderErrorCode::GenerationBusy => error(ErrorCode::ProviderBusy, message),
        ProviderErrorCode::Cancelled => error(ErrorCode::Cancelled, message),
        ProviderErrorCode::ContextLimit => error(ErrorCode::ContextOverflow, message),
        ProviderErrorCode::EmbeddingSpaceMismatch => {
            let mut result = error(ErrorCode::EmbeddingSpaceMismatch, message);
            if let Some(detail) = detail.as_deref() {
                for pair in detail.split(';') {
                    let Some((key, value)) = pair.split_once('=') else {
                        continue;
                    };
                    if matches!(key, "expected" | "actual") && !value.is_empty() {
                        result = result.with_detail(key, value);
                    }
                }
            }
            result
        }
        ProviderErrorCode::InvalidModelOutput => error(ErrorCode::Internal, message)
            .with_detail("reportedCode", "invalidModelOutput")
            .with_detail("outputDigest", digest_text(detail.as_deref().unwrap_or(""))),
        ProviderErrorCode::NoEvidence => {
            error(ErrorCode::Internal, message).with_detail("reportedCode", "noEvidence")
        }
        ProviderErrorCode::IoError => {
            let mut result =
                error(ErrorCode::Internal, message).with_detail("reportedCode", "ioError");
            if let Some(path) = detail {
                result = result.with_detail("path", path);
            }
            result
        }
    }
}

pub(crate) fn core_failure(failure: CoreError) -> FolioError {
    match failure {
        CoreError::Provider(provider) => provider_failure(NativeProviderError {
            code: provider.code,
            message: provider.message,
            detail: provider.detail,
        }),
        CoreError::Io(failure) => {
            error(ErrorCode::Internal, failure.to_string()).with_detail("reportedCode", "ioError")
        }
        CoreError::Http(failure) => {
            let host = failure
                .url()
                .and_then(|url| url.host_str())
                .map(str::to_owned);
            let mut result = error(ErrorCode::Internal, failure.to_string())
                .with_detail("reportedCode", "downloadFailed");
            if let Some(host) = host {
                result = result.with_detail("url", host);
            }
            result
        }
        CoreError::Json(failure) => {
            error(ErrorCode::Internal, failure.to_string()).with_detail("reportedCode", "ioError")
        }
        CoreError::Archive(message) | CoreError::Message(message) => {
            error(ErrorCode::Internal, message).with_detail("reportedCode", "ioError")
        }
    }
}

impl From<NativeProviderError> for FolioError {
    fn from(failure: NativeProviderError) -> Self {
        provider_failure(failure)
    }
}

impl From<CoreError> for FolioError {
    fn from(failure: CoreError) -> Self {
        core_failure(failure)
    }
}

fn digest_text(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn unknown_workspace_is_rejected_by_the_native_registry() {
        let data = tempfile::tempdir().unwrap();
        let state = Folio::open(data.path().join("folio.sqlite")).unwrap();
        let error = resolve_workspace(&state, "made-up-workspace").unwrap_err();
        assert_eq!(error.code, crate::error::ErrorCode::WorkspaceNotAuthorized);
    }

    #[test]
    fn authorized_workspace_resolves_to_the_registry_root() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("notes.md"), "notes").unwrap();
        let data = tempfile::tempdir().unwrap();
        let state = Folio::open(data.path().join("folio.sqlite")).unwrap();
        let info = state
            .workspaces
            .lock()
            .unwrap()
            .authorize(root.path())
            .unwrap();

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

    #[test]
    fn provider_codes_map_to_frozen_wire_codes() {
        let cases = [
            (
                ProviderErrorCode::ModelNotInstalled,
                ErrorCode::ModelNotInstalled,
            ),
            (ProviderErrorCode::ModelCorrupt, ErrorCode::ModelLoadFailed),
            (
                ProviderErrorCode::RuntimeMissing,
                ErrorCode::ModelNotInstalled,
            ),
            (
                ProviderErrorCode::RuntimeStartFailed,
                ErrorCode::ModelLoadFailed,
            ),
            (ProviderErrorCode::GenerationBusy, ErrorCode::ProviderBusy),
            (ProviderErrorCode::Cancelled, ErrorCode::Cancelled),
            (ProviderErrorCode::ContextLimit, ErrorCode::ContextOverflow),
            (
                ProviderErrorCode::EmbeddingSpaceMismatch,
                ErrorCode::EmbeddingSpaceMismatch,
            ),
            (ProviderErrorCode::InvalidModelOutput, ErrorCode::Internal),
            (ProviderErrorCode::NoEvidence, ErrorCode::Internal),
            (ProviderErrorCode::IoError, ErrorCode::Internal),
        ];
        for (code, expected) in cases {
            assert_eq!(
                provider_failure(NativeProviderError {
                    code,
                    message: "failure".into(),
                    detail: None,
                })
                .code,
                expected
            );
        }
    }

    #[test]
    fn provider_mapping_preserves_sanctioned_details() {
        let runtime = provider_failure(NativeProviderError {
            code: ProviderErrorCode::RuntimeMissing,
            message: "runtime missing".into(),
            detail: Some("llama-runtime".into()),
        });
        assert_eq!(runtime.code, ErrorCode::ModelNotInstalled);
        assert_eq!(runtime.detail("component"), Some("runtime"));
        assert_eq!(runtime.detail("runtimeId"), Some("llama-runtime"));

        let space = provider_failure(NativeProviderError {
            code: ProviderErrorCode::EmbeddingSpaceMismatch,
            message: "space mismatch".into(),
            detail: Some("expected=folio-space-v1/a;actual=folio-space-v1/b".into()),
        });
        assert_eq!(space.detail("expected"), Some("folio-space-v1/a"));
        assert_eq!(space.detail("actual"), Some("folio-space-v1/b"));

        let invalid = provider_failure(NativeProviderError {
            code: ProviderErrorCode::InvalidModelOutput,
            message: "invalid output".into(),
            detail: Some("raw output".into()),
        });
        assert_eq!(invalid.code, ErrorCode::Internal);
        assert_eq!(invalid.detail("reportedCode"), Some("invalidModelOutput"));
        assert!(invalid.detail("outputDigest").is_some());
    }

    #[test]
    fn core_failures_use_internal_unknown_code_details() {
        let failure = core_failure(CoreError::Provider(NativeProviderErrorError::new(
            ProviderErrorCode::InvalidModelOutput,
            "bad output",
        )));
        assert_eq!(failure.code, ErrorCode::Internal);
        assert_eq!(failure.detail("reportedCode"), Some("invalidModelOutput"));
    }
}
