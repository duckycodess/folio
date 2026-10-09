//! Pure Rust seams for Folio's local providers and Model Lab.
//!
//! The Tauri application crate owns commands and user-folder permissions. This
//! crate deliberately has no Tauri dependency so its safety and contract logic
//! can run in the Linux CI job without WebKitGTK.

pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod contracts;
