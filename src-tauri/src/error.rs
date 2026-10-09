use serde::Serialize;

/// Stable error codes shared with the UI (`NativeErrorCode` in `src/domain/contracts.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    PathEscape,
    NotAuthorized,
    NotFound,
    Unsupported,
    TooLarge,
    InvalidInput,
    Busy,
    EmbeddingSpaceMismatch,
    /// A target's bytes no longer match the preview; a new preview is required.
    TargetChanged,
    /// The index is behind the file on disk; refresh before planning a change.
    StaleIndex,
    AmbiguousEdit,
    UnsupportedEdit,
    Collision,
    ApprovalRequired,
    PlanExpired,
    /// The approved digest does not match the stored plan.
    PlanChanged,
    PlanState,
    UndoConflict,
    UndoUnavailable,
    Io,
    Database,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeError {
    pub code: ErrorCode,
    pub message: String,
}

pub type NativeResult<T> = Result<T, NativeError>;

pub fn fail(code: ErrorCode, message: impl Into<String>) -> NativeError {
    NativeError { code, message: message.into() }
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl From<rusqlite::Error> for NativeError {
    fn from(error: rusqlite::Error) -> Self {
        fail(ErrorCode::Database, error.to_string())
    }
}

impl From<std::io::Error> for NativeError {
    fn from(error: std::io::Error) -> Self {
        fail(ErrorCode::Io, error.to_string())
    }
}

impl From<serde_json::Error> for NativeError {
    fn from(error: serde_json::Error) -> Self {
        fail(ErrorCode::Io, error.to_string())
    }
}
