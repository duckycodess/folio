# Implementation status

## Implemented starter pieces

- SOS React interface with workflows A/B/C and synthetic document navigation.
- Keyword filtering (explicitly labelled), actual Markdown-link discovery in fixture text, source content views, and a graph of those explicit links.
- Tauri folder picker and scoped native listing/TXT/Markdown reading commands.
- Shared document, evidence, relationship, action-plan, provider, and benchmark contracts.
- Deterministic approval-state logic and a SQLite migration contract.
- Fifteen English/Filipino/Taglish synthetic text documents and labelled benchmark cases.
- Product docs, glossary, ADRs, four-person plan, and CI definitions.

## Pending

SQLite connection, text-PDF extraction, shared-fact discovery, durable apply/undo/history, incremental persisted indexing, native packaging, and real Model Lab results remain pending. Issue #4 now supplies interim in-memory retrieval and local-provider seams without replacing #3 persistence or #5 approval.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Issue #4 planning follow-up

Recorded accepted model-selection, independent acceptance-suite, clarification, and full/partial-summary policies in `docs/grill-with-docs.md`; added Partial Summary vocabulary to `GLOSSARY.md`. The combined issues #4/#8 follow-up also settled objective checks plus TJ factual review (unreviewed summaries are not passes), and disposable benchmark workspaces with native approval for actual apply tests. Earlier Q5–Q7 proposals remain unaccepted. The follow-up itself changed documentation only; the later #4 implementation is recorded below.

## Verification

Checked on 2026-10-09 with Node.js 24.16.0 and npm 11.13.0:

- `npm run check`: passed.
- `npm test`: 11 tests passed, covering approval/expiry/current-content checks, required history, explicit-reference containment, Filipino keyword retrieval, contract goldens, and embedding-space fingerprints.
- `npm run build`: passed; the production frontend compiles.
- SQLite migration: executed in memory with Python's SQLite; Filipino FTS matching, insert/update/delete triggers, embedding revision separation, foreign-key guards, and cascading cleanup passed.
- `npm run tauri info`: configuration recognized. Earlier attempts selected system Cargo 1.75 and failed to parse the lockfile. User-local Rust/Cargo 1.96.1 is available via an explicit PATH; WSL desktop prerequisites were subsequently installed. Windows/macOS CI jobs are configured but have not run here.
- Browser smoke checks were attempted but could not start: the browser binary is absent and its download returned an invalid archive. Visual behavior and native folder-picker behavior still require verification.

Issue #4 local inference/provider smoke evidence is now recorded below. PDF extraction, filesystem apply/undo, native packaging, and actual performance/size measurements remain outside the verified scope.

## Issue #4 implementation evidence (2026-10-09)

Issue #4 is implemented at the pure-core/provider and native-command-adapter level, but is not represented as fully verified or complete. The implementation is on branch `FOLIO-4`; no `FOLIO-8` branch, Model Lab benchmark/persistence harness, or benchmark corpus was created. The user subsequently requested publication of the current work without further WSL testing; it is being submitted as a draft PR, not a completion claim. The separate ignored R8 acceptance harness is not #8 work.

Implemented #4 slices:

- `folio-core` is a pure Rust workspace member with shared camelCase contracts and JSON goldens.
- The pinned model manifest supports explicit model/runtime install, size plus SHA-256 verification, atomic partial-file replacement, selection, verification, removal, and archive/path/symlink safety under app data.
- ONNX multilingual E5 preprocessing, embedding-space fingerprints, UTF-16 interim chunking, exact vector isolation, and keyword/semantic/hybrid result labels are implemented. The interim index is in memory until #3 supplies persisted chunks/FTS5.
- The llama.cpp provider uses fixed loopback-only launch arguments, a random local API key, schema-constrained JSON requests, one active generation, cancellation checks, timeout, idle reuse/unload, and verified model/runtime paths.
- Grounded map/reduce summaries and answers validate citations, count uncited sentences, report coverage, return bounded partial summaries, and avoid generation when evidence is absent.
- Interpretation is request-only at the model boundary, then deterministic: exact-stem resolution, ambiguous selection, current-content exact-find validation, duplicate-path information, safe TXT/Markdown destinations, and proposal-only typed operations. There is no #5 approval/apply/write path.
- `src/adapters/models.ts` and `src/adapters/ai.ts` are thin invoke wrappers. Browser preview AI calls raise typed `runtimeMissing`; fixtures are not labelled inference.

Deterministic and provider evidence available around the WSL restart:

- Before the review fixes, the pure-core suite at reviewed SHA `e0efdce` passed 33 tests with 0 failures and 2 ignored tests. Subsequent native compilation used user-local Cargo 1.96.1. Codex reported 5 Tauri app-library tests passing after the review fixes; later compilation fixes and interrupted builds have not received a complete final verification run.
- Fresh manifest-pinned files were downloaded into disposable `/tmp/folio-4-recheck.Fx1E4q` and verified before use: E5 ONNX 118,308,185 bytes (`f80102d3f2a1229f387d3c81909990d8945513e347b0eab049f7de3c6f98c193`), tokenizer 17,082,730 bytes (`0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39`), Qwen3-0.6B Q4 396,705,472 bytes (`ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a`), and llama.cpp b11524 archive 17,831,545 bytes (`2f44f8e794a9e6290bacb2b811e6a83019a106c057473682aeeb445bd49c5083`). The extracted `llama-server --version` reported build 11524 on Linux x86_64.
- With those fresh files, `cargo test --manifest-path src-tauri/crates/folio-core/Cargo.toml -- --ignored` passed 2/2 normally and passed 2/2 inside `unshare -rn` with loopback enabled. The E5 smoke produced a 384-dimensional L2-normalized vector; the llama smoke launched the local server and received schema-valid JSON. These are provider/smoke results, not subjective answer correctness or benchmark results.
- A real-provider R8 harness now covers labelled English→Filipino, Filipino→English, Taglish retrieval, both fixed deadline interpretation phrasings, ambiguity, the deterministic no-evidence gate, cited summary output/fact checks, cancellation, and post-cancellation generation. It is deliberately `#[ignore]`, uses the disposable fixture corpus only as input, and has not received a completed current-checkout run; further local testing was stopped at the user's request after repeated WSL crashes.
- WSL `npm run check`, `npm test -- --run` (11 tests), and `npm run build` passed. Rust formatting checks for changed core/native files and `git diff --check` passed.
- A new isolated Windows copy at `C:\Users\Aeron\AppData\Local\Temp\folio-codex-4-recheck-20261009` excluded `.git`, dependencies, targets, model files, and generated output. Windows Node 22.20.0/npm 10.9.3 ran `npm.cmd ci`, `npm.cmd run check`, `npm.cmd test -- --run` (11 tests), and `npm.cmd run build`: all passed.
- WSL desktop prerequisites were installed with user authorization through WSL's root-user entry point. Verified versions: WebKitGTK/JavaScriptCoreGTK 2.52.6, libsoup 3.4.4, librsvg 2.58.0. The missing-library blocker is resolved; repeated WSL restarts interrupted subsequent build verification.
- Windows hardware and toolchain facts are verified: ASUS TUF Gaming A15, 16,371,474,432 bytes physical RAM, PowerShell interop, Windows Node, Cargo/Rust 1.82.0, and Visual Studio Community 2022 with VC.Tools.x86.x64. Windows `cargo test --manifest-path src-tauri/Cargo.toml` stops before compilation because Cargo 1.82 cannot parse cached `base64ct-1.8.3` requiring stabilized `edition2024`; no toolchain/global configuration was changed.

Not yet evidenced: a complete final-checkout Rust test/build run, task-level real-model acceptance after the fixes, model/runtime integrated downloads on all target platforms, Tauri desktop launch or UI interaction, Windows Rust-native build with a current Cargo, macOS build/run, packaging/sidecar/Windows Job Object verification, persistent #3 index integration, #5 approval/apply/undo, 8 GB target measurements, and subjective TJ review of any generated answer. Q5–Q7 remain provisional and unaccepted. The initial review required fixes; the final revision has not received completed re-review. At the user's request, further WSL tests/builds/inference are stopped and the current work is being published as a draft, partial #4 PR. No tests were run for this documentation update; merge readiness and issue completion are not claimed.
