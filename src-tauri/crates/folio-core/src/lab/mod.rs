//! Model Lab: the fixed-suite harness and its result records.
//!
//! Pure Rust with no database or Tauri dependency. The harness reports through
//! a [`LabSink`]; the application crate persists to SQLite and CI writes JSON.

pub mod checks;
pub mod host;
pub mod memory;
pub mod record;
pub mod sink;
pub mod suite;
pub mod workspace;

pub use record::*;
pub use sink::{LabSink, MemorySink};
