# Development setup

## Frontend

Install a supported Node.js release (22.12+ is the baseline), clone/unpack the repository, and run `npm ci`, then `npm run dev`. `npm run check`, `npm test`, and `npm run build` verify the frontend and deterministic domain logic.

The browser preview reads only synthetic fixtures. It does not grant local-folder access, run semantic embeddings, generate summaries, or save edits.

## Desktop

Install [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, including the Rust stable toolchain. Windows needs the documented C++ build tools and WebView2 prerequisites; macOS needs the documented Xcode command-line tools. Run `npm run tauri dev`. Select a folder through the native folder picker. The backend indexes TXT, Markdown and text-based PDFs in that folder into `folio.sqlite` in the OS application-data directory, with SQLite FTS5 keyword search. Folders chosen in the picker can be reopened in later sessions without the dialog. Changes made outside Folio are picked up on the next scan; there is no live file watcher.

Use `npm run tauri build` on the intended target OS to compile. Native code and installers need actual target-platform verification. The initial bundle configuration is disabled until runtime/model packaging and platform installer assets are integrated; CI native checks compile/test the shell but do not publish a consumer installer. Source SVG and PNG/ICO window icons are included.

## WSL

Use WSL for frontend and shared checks if convenient. Develop and verify the Windows native shell with Windows toolchains. Keep one compatible model store per installation rather than separately downloading weights into Windows, WSL, and Folio. Do not commit WSL caches or native build targets.

## Local models

Weights and inference binaries are deliberately excluded from Git. Model downloads will be explicit, user-triggered setup actions with revision/hash and disk size displayed. The default footprint must include tokenizer and runtime files. Reference `docs/architecture.md` for candidate models and runtime adapters.

Issue #4 connects the local providers in `folio-core`: in-process ONNX Runtime embeddings and a pinned llama.cpp `llama-server` child bound to `127.0.0.1`. Ollama is not used; installing it does not connect it to Folio. AI actions stay behind explicit model setup and are not yet reachable from the UI.

## Issue #4 local provider development

Run Rust commands with the rustup toolchain first on hosts where `/usr/bin/cargo` is older:

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo test --manifest-path src-tauri/crates/folio-core/Cargo.toml
```

The pure `folio-core` tests are the portable deterministic gate. The full Tauri crate additionally needs the target desktop libraries (`webkit2gtk-4.1`, `libsoup-3.0`, `javascriptcoregtk-4.1` and `librsvg2` development packages on Linux). Install them only with the user's authorization, then run:

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo test --manifest-path src-tauri/Cargo.toml
```

Windows verification uses a disposable copy with `.git`, `node_modules`, build targets, model files, and generated output excluded. The verified machine has Windows Node/npm, Rust/Cargo, Visual Studio Community with VC.Tools.x86.x64, and working PowerShell interop. The native shell still needs a successful current Windows Rust/Cargo build and a real desktop launch before claiming Windows interaction verification. macOS requires an actual macOS build/run.

**Build requirements.** `folio-core` depends on `ort` (pinned exactly to `2.0.0-rc.13`). Its `ort-sys` build script downloads prebuilt ONNX Runtime binaries during the first `cargo build`, so that build needs network access. On Linux it also needs the OpenSSL development headers (`libssl-dev`), because the download uses `ureq` with `native-tls`. On the Linux CI runner the test binary linked ONNX Runtime statically (no separate library to ship); Windows and macOS linkage has not been inspected. This build-time download is a second supply-chain path next to the hash-pinned runtime manifest. Shipping ONNX Runtime through the verified manifest instead (`ort`'s `load-dynamic` feature) is an open option.

Models and runtimes are installed only through the explicit native commands into app data. The pinned embedding files and Qwen/llama.cpp assets are listed in `src-tauri/resources/model-manifest.json`. Downloads are HTTPS-only, capped at the manifest size and verified by byte count and SHA-256; model files are then renamed atomically. A runtime archive is extracted into a staging directory (versioned shared-library links allowed only as siblings), recorded with the SHA-256 of every extracted file, and swapped into place; the executable is verified against that record before each launch. A model file, runtime binary, or generated answer in a fixture test is not an inference result. Real provider smoke tests must identify the model revision, runtime, host, and whether output was reviewed.

The browser preview intentionally rejects AI operations with a `modelNotInstalled` failure (`details.component: "runtime"`, `details.reason: "browserPreview"`). It never presents fixture text as local inference. #4 does not run Model Lab or write benchmark records; that remains the strictly sequential disposable-corpus work in #8 after the #4 review/merge sequence.

## Publishing the prepared starter

The intended new repository is `duckycodess/folio`, public. Do not overwrite an existing repository without checking its contents. If GitHub CLI is available and authenticated, the repository creation command is:

```bash
gh repo create duckycodess/folio --public --description "Search, Organize, Summarize — a private local-AI file workspace" --source . --remote origin --push
```

The prepared working tree is committed locally. The archive contains its source snapshot and excludes `.git`; initialize and commit it first if using the archive. Never put tokens into remote URLs or commit real documents. Public creation is authorized by the user's request; use an available authenticated creation method.
