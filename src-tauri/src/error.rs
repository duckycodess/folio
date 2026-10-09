use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Every failure that crosses the UI boundary carries one of these codes.
///
/// The TypeScript union in `src/domain/contracts.ts` lists the same values in
/// the same order, and both are checked against
/// `fixtures/contracts/contract-cases.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    WorkspaceNotAuthorized,
    WorkspaceUnavailable,
    PathNotRelative,
    PathEscapesWorkspace,
    PathUnsupportedEncoding,
    DocumentUnavailable,
    DocumentTooLarge,
    DocumentNotText,
    UnsupportedMediaType,
    PlanUnknown,
    PlanEmpty,
    PlanExpired,
    PlanStateInvalid,
    PlanDigestMismatch,
    ApprovalRequired,
    ApprovalStale,
    DuplicateOperationTarget,
    TargetMissing,
    TargetChanged,
    DestinationExists,
    OperationUnsupported,
    HistoryRequired,
    HistoryUnknown,
    UndoConflict,
    WriterNotImplemented,
    ModelNotInstalled,
    ModelLoadFailed,
    ProviderBusy,
    Cancelled,
    ContextOverflow,
    EmbeddingSpaceMismatch,
    EvidenceInvalid,
    Internal,
}

/// The frozen list, in wire order. Used by the cross-language fixture test.
#[allow(dead_code)]
pub const ALL_ERROR_CODES: [ErrorCode; 33] = [
    ErrorCode::WorkspaceNotAuthorized,
    ErrorCode::WorkspaceUnavailable,
    ErrorCode::PathNotRelative,
    ErrorCode::PathEscapesWorkspace,
    ErrorCode::PathUnsupportedEncoding,
    ErrorCode::DocumentUnavailable,
    ErrorCode::DocumentTooLarge,
    ErrorCode::DocumentNotText,
    ErrorCode::UnsupportedMediaType,
    ErrorCode::PlanUnknown,
    ErrorCode::PlanEmpty,
    ErrorCode::PlanExpired,
    ErrorCode::PlanStateInvalid,
    ErrorCode::PlanDigestMismatch,
    ErrorCode::ApprovalRequired,
    ErrorCode::ApprovalStale,
    ErrorCode::DuplicateOperationTarget,
    ErrorCode::TargetMissing,
    ErrorCode::TargetChanged,
    ErrorCode::DestinationExists,
    ErrorCode::OperationUnsupported,
    ErrorCode::HistoryRequired,
    ErrorCode::HistoryUnknown,
    ErrorCode::UndoConflict,
    ErrorCode::WriterNotImplemented,
    ErrorCode::ModelNotInstalled,
    ErrorCode::ModelLoadFailed,
    ErrorCode::ProviderBusy,
    ErrorCode::Cancelled,
    ErrorCode::ContextOverflow,
    ErrorCode::EmbeddingSpaceMismatch,
    ErrorCode::EvidenceInvalid,
    ErrorCode::Internal,
];

/// The wire form of a failure. A command returns `Result<T, FolioError>`, so the
/// UI always receives a code it can act on together with prose for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolioError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<BTreeMap<String, String>>,
}

impl FolioError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details
            .get_or_insert_with(BTreeMap::new)
            .insert(key.into(), value.into());
        self
    }

    #[allow(dead_code)]
    pub fn detail(&self, key: &str) -> Option<&str> {
        self.details.as_ref()?.get(key).map(String::as_str)
    }
}

impl fmt::Display for FolioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for FolioError {}

pub fn error(code: ErrorCode, message: impl Into<String>) -> FolioError {
    FolioError::new(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_codes_in_camel_case() {
        let value = serde_json::to_value(ErrorCode::PathEscapesWorkspace).unwrap();
        assert_eq!(value, serde_json::json!("pathEscapesWorkspace"));
    }

    #[test]
    fn serializes_a_failure_with_its_details() {
        let failure = error(ErrorCode::TargetChanged, "This file changed.")
            .with_detail("path", "projects/project-plan.md");
        let value = serde_json::to_value(&failure).unwrap();
        assert_eq!(value["code"], serde_json::json!("targetChanged"));
        assert_eq!(value["message"], serde_json::json!("This file changed."));
        assert_eq!(
            value["details"]["path"],
            serde_json::json!("projects/project-plan.md")
        );
    }

    #[test]
    fn omits_absent_details_rather_than_sending_null() {
        let value = serde_json::to_value(error(ErrorCode::Internal, "x")).unwrap();
        assert!(value.get("details").is_none());
    }
}
