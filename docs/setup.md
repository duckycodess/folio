# Development setup

## Frontend

Install a supported Node.js release (22.12+ is the baseline), clone/unpack the repository, and run `npm ci`, then `npm run dev`. `npm run check`, `npm test`, and `npm run build` verify the frontend and deterministic domain logic.

The browser preview reads only synthetic fixtures. It does not grant local-folder access, run semantic embeddings, generate summaries, or save edits.

## Desktop

Install [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, including the Rust stable toolchain. Windows needs the documented C++ build tools and WebView2 prerequisites; macOS needs the documented Xcode command-line tools. Run `npm run tauri dev`. Select a folder through the native folder picker. The current backend lists supported files and reads TXT/Markdown within that authorized root; PDF extraction is pending.

Use `npm run tauri build` on the intended target OS to compile. Native code and installers need actual target-platform verification. The initial bundle configuration is disabled until runtime/model packaging and platform installer assets are integrated; CI native checks compile/test the shell but do not publish a consumer installer. Source SVG and PNG/ICO window icons are included.

## WSL

Use WSL for frontend and shared checks if convenient. Develop and verify the Windows native shell with Windows toolchains. Keep one compatible model store per installation rather than separately downloading weights into Windows, WSL, and Folio. Do not commit WSL caches or native build targets.

## Local models

Weights and inference binaries are deliberately excluded from Git. Model downloads will be explicit, user-triggered setup actions with revision/hash and disk size displayed. The default footprint must include tokenizer and runtime files. Reference `docs/architecture.md` for candidate models and runtime adapters.

No runtime adapter is connected in this starter. Installing Ollama alone does not connect it to Folio. Implement T2 against the existing provider interfaces, use loopback-only endpoints, and measure on the fixed suite before enabling AI actions.

## Publishing the prepared starter

The intended new repository is `duckycodess/folio`, public. Do not overwrite an existing repository without checking its contents. If GitHub CLI is available and authenticated, the repository creation command is:

```bash
gh repo create duckycodess/folio --public --description "Search, Organize, Summarize — a private local-AI file workspace" --source . --remote origin --push
```

The prepared working tree is committed locally. The archive contains its source snapshot and excludes `.git`; initialize and commit it first if using the archive. Never put tokens into remote URLs or commit real documents. Public creation is authorized by the user's request; use an available authenticated creation method.
