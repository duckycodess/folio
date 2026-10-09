# Use a small native desktop core with a shared SOS interface

Windows/macOS packaging and the default installation target favor Tauri with a React/TypeScript interface and a small Rust core that owns file authorization and mutations. This introduces native toolchain integration into the short build window, so verify native boot on both target platforms immediately and keep frontend development unblocked through the fixture adapter. This is a starter architecture choice, not a claim of completed cross-platform verification.
