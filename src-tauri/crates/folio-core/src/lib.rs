//! Pure Rust seams for Folio's local providers and Model Lab.
//!
//! The Tauri application crate owns commands and user-folder permissions. This
//! crate deliberately has no Tauri dependency so its safety and contract logic
//! can run in the Linux CI job without WebKitGTK.

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod chunking;
pub mod collections;
pub mod contracts;
pub mod device;
pub mod embeddings;
pub mod error;
pub mod facts;
pub mod generation;
pub mod grounding;
pub mod interpretation;
pub mod lab;
pub mod models;
pub mod relationships;
pub mod retrieval;
