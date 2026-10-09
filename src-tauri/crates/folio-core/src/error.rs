use crate::contracts::{NativeProviderError, ProviderErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Provider(#[from] NativeProviderErrorError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("archive error: {0}")]
    Archive(String),
    /// A caller's Stop was honoured at a safe point; completed work is kept.
    #[error("relationship discovery cancelled")]
    Cancelled,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct NativeProviderErrorError {
    pub code: ProviderErrorCode,
    pub message: String,
    pub detail: Option<String>,
}

impl NativeProviderErrorError {
    pub fn new(code: ProviderErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn into_native(self) -> NativeProviderError {
        NativeProviderError {
            code: self.code,
            message: self.message,
            detail: self.detail,
        }
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
