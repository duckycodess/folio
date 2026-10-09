# Development setup

## Frontend

Install a supported Node.js release (22.12+ is the baseline), clone/unpack the repository, and run `npm ci`, then `npm run dev`. `npm run check`, `npm test`, and `npm run build` verify the frontend and deterministic domain logic.

The browser preview reads only synthetic fixtures. It does not grant local-folder access, run semantic embeddings, generate summaries, or save edits.

## Desktop

Install [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, including the Rust stable toolchain. Windows needs the documented C++ build tools and WebView2 prerequisites; macOS needs the documented Xcode command-line tools. Run `npm run tauri dev`. Select a folder through the native folder picker. The backend indexes TXT, Markdown and text-based PDFs in that folder into `folio.sqlite` in the OS application-data directory, with SQLite FTS5 keyword search. Folders chosen in the picker can be reopened in later sessions without the dialog. Changes made outside Folio are picked up on the next scan; there is no live file watcher.

Use `npm run tauri build` on the intended target OS to compile. Native code and installers need actual target-platform verification. The default bundle configuration remains disabled. A dedicated manual packaging override prepares test installers; see the installer preparation section below. Existing native test CI compiles/tests the shell and does not build a consumer installer. Source SVG and PNG/ICO window icons are included.

## WSL

Use WSL for frontend and shared checks if convenient. Develop and verify the Windows native shell with Windows toolchains. Keep one compatible model store per installation rather than separately downloading weights into Windows, WSL, and Folio. Do not commit WSL caches or native build targets.

## Local models

Weights and inference binaries are deliberately excluded from Git. Model downloads will be explicit, user-triggered setup actions with revision/hash and disk size displayed. The default footprint must include tokenizer and runtime files. Reference `docs/architecture.md` for candidate models and runtime adapters.

Issue #4 connects the local providers in `folio-core`: in-process ONNX Runtime embeddings and a pinned llama.cpp `llama-server` child bound to `127.0.0.1`. Ollama is not used; installing it does not connect it to Folio. Model Lab provides explicit model/runtime installation, cancellation, selection and removal. AI-dependent actions still require setup; packaged first-run setup and full offline integration remain unverified.

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

The browser preview intentionally rejects AI operations with a `modelNotInstalled` failure (`details.component: "runtime"`, `details.reason: "browserPreview"`). It never presents fixture text as local inference. Model Lab (#8) is native-only: its commands (`lab_models`, `install_lab_candidate`, `remove_lab_candidate`, `verify_lab_candidate`, `run_model_lab`, `cancel_model_lab`, `list_lab_runs`, `list_lab_results`, `record_lab_review`) reject in the browser preview and nothing in the preview is shown as a measurement.

**Measuring on a hosted runner.** `.github/workflows/model-lab.yml` is `workflow_dispatch` only. It takes a runner (`windows-latest` by default, `macos-latest` opt-in), the embedding model id and a comma-separated list of generation model ids (default: the smallest pair, `multilingual-e5-small-int8` + `qwen3-0.6b-q4-k-m`). It checks the ids against the pinned manifest and the free disk before downloading, installs through `ModelStore` (size and SHA-256 verified), runs the fixed suite and uploads the records as a JSON artifact with a summary table. There is no download byte cap; the manifest byte totals are printed for information. No model or platform is substituted for one that was not requested. Six evaluation-only candidates (`qwen3.5-0.8b-q4-k-m`, `qwen3.5-2b-q4-k-m`, `gemma-sea-lion-v4.5-e2b-q4-k-m`, `gemma-4-e2b-q4-k-m`, `ministral-3-3b-q4-k-m`, `lfm2.5-1.2b-q4-k-m`, see [model candidates](model-candidates.md)) are accepted by name in `generation_models`; they are never run by default, are not supported or recommended models, the SEA-LION license metadata is unsettled, and LFM2.5 uses the publisher's own LFM Open License v1.0. The lab launches every llama-server with `--n-gpu-layers 0 --device none` (the target is CPU inference) and records the backend the server itself printed; a model that cannot start is recorded as a startup failure and the run continues. A model getting a case wrong is recorded data, not a failed job. These are hosted-runner measurements: not an 8 GB device, not installed size, and not desktop or GUI evidence.

## Publishing the prepared starter

The intended new repository is `duckycodess/folio`, public. Do not overwrite an existing repository without checking its contents. If GitHub CLI is available and authenticated, the repository creation command is:

```bash
gh repo create duckycodess/folio --public --description "Search, Organize, Summarize — a private local-AI file workspace" --source . --remote origin --push
```

The prepared working tree is committed locally. The archive contains its source snapshot and excludes `.git`; initialize and commit it first if using the archive. Never put tokens into remote URLs or commit real documents. Public creation is authorized by the user's request; use an available authenticated creation method.

## Installer preparation

Issue #10 adds a manual Windows x64 / Apple Silicon macOS packaging workflow through an explicit configuration override. Default bundling remains disabled. These artifacts are unsigned Windows / ad-hoc macOS test builds, not a verified consumer release. No model weights or llama.cpp downloads are added. See [packaging instructions and later clean-machine validation](packaging.md) for build commands, artifact checksums, evidence limits and pending native/offline/resource checks.
