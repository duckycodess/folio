# Installer preparation and later validation

The **Installer preparation** workflow is manual (`workflow_dispatch`). Its narrow targets are Windows x64 (`x86_64-pc-windows-msvc`, NSIS setup executable) and Apple Silicon macOS (`aarch64-apple-darwin`, DMG). Intel macOS and Windows ARM64 are not covered by this slice. A new dispatch workflow must first exist on the default branch; until that human merge, installer build results remain pending. Once available, open GitHub Actions, select the workflow and choose the commit/branch to prepare. It does not run on ordinary UI pull requests and does not publish GitHub Releases.

`src-tauri/tauri.packaging.conf.json` is an explicit CLI override; the default `tauri.conf.json` still has bundling disabled. Only Cargo registry/Git downloads are cached; generated targets and installers are never restored from cache. The collector refuses existing installers before a build and requires one fresh output matching the recorded commit, target and start time. The packaging workflow derives icons from the committed source SVG into the ignored native target directory. It builds with the npm/Cargo lockfiles, records actual tool versions, and uploads the installer with SHA-256 checksums and `build-metadata.json`. Downloads expire after seven days; record the exact run and commit before sharing a demo artifact.

These are preparation artifacts, not a verified consumer release. Windows installers have no Authenticode signature. macOS apps use a credential-free ad-hoc signature (`signingIdentity: "-"`), with no Apple-authenticated identity or notarization. They can be blocked by operating-system security checks; stop and record the exact message instead of disabling security controls. Credentials, signing identities, release publication and updater packaging are outside this slice.

No model weights or llama.cpp archives are bundled or explicitly fetched by this workflow. Existing native first-run installation commands use the revision/hash-pinned manifest and application-data store. Model Lab already exposes explicit install, progress, cancel, remove and selection controls; those controls have not been verified in these installers. The native build itself can download ONNX Runtime through `ort-sys`; Windows/macOS runtime linkage, necessary redistributed libraries and third-party notices still require inspection. A successful hosted build alone does not prove the package boots on a clean machine.

Local preparation uses an actual target host with the documented Tauri prerequisites:

```bash
npm ci
npm run tauri -- icon src-tauri/icons/source.svg --output src-tauri/target/packaging-icons
# Windows x64:
npm run tauri -- build --ci --config src-tauri/tauri.packaging.conf.json --target x86_64-pc-windows-msvc --bundles nsis -- --locked
# Apple Silicon macOS:
npm run tauri -- build --ci --config src-tauri/tauri.packaging.conf.json --target aarch64-apple-darwin --bundles dmg -- --locked
```

Use only the command matching the host. These commands require build-time network access. Windows' unchanged WebView2 bootstrapper behavior may also require network during initial installation; offline operation is tested after explicit setup, not assumed from the existence of an installer.

### Later clean-machine validation

For each installer, retain the run URL, commit, artifact SHA-256, target/OS version, CPU, RAM, Node/Rust/Tauri versions and model/runtime revisions. Every unchecked item stays **Not verified**:

1. Verify the artifact checksum. Install on the corresponding clean target machine; record prerequisite/security dialogs and missing DLL/library failures. Launch and confirm a real Folio window, then exercise the native folder picker against a disposable copy of `fixtures/documents`.
2. In Model Lab, perform explicit setup/downloads, checking displayed revisions, byte sizes, failures/cancellation and resume/retry behavior. No fixture text counts as inference. Restart Folio and verify folder/model selection persistence without resumed mutations.
3. Disconnect internet after setup. Run native A/B/C and the full Taglish demo in `docs/acceptance.md`, including source evidence, preview/approval, hashes of changed/unchanged files, Ripple candidates, real history and conflict-aware Undo. Browser fake-core tests are separate evidence.
4. Measure installed app, shipped/runtime files, tokenizer and default weights separately from originals, SQLite/index, caches and history. Installer compressed bytes are not installed footprint. Preserve a file inventory and actual sizes; compare the combined default installation against the under-1-GB target.
5. On an 8-GB laptop without a dedicated GPU, record cold/warm load, peak memory with each measured process identified, and search latency while generation runs. Hosted runner figures do not satisfy this hardware check.
6. Exercise relaunch, installation of the next prepared version and removal using the target platform. Record what happens to application-data models, indexes and history. Use synthetic folders and verify originals survive. Record licensing/redistribution review and Windows/macOS runtime linkage before any consumer-release claim.

Issue #10 remains open. Its full offline/release acceptance still depends on the native integration evidence in #9; the fake-core UI suite alone does not satisfy that dependency.

Sources: [Tauri CLI overrides](https://v2.tauri.app/reference/cli/#build), [Windows installers](https://v2.tauri.app/distribute/windows-installer/), [macOS ad-hoc signing](https://v2.tauri.app/distribute/sign/macos/#ad-hoc-signing), [app icons](https://v2.tauri.app/develop/icons/), and [GitHub runner architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
