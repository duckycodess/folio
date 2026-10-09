# Implementation status

## Implemented starter pieces

- SOS React interface with workflows A/B/C and synthetic document navigation.
- Keyword filtering (explicitly labelled), actual Markdown-link discovery in fixture text, source content views, and a graph of those explicit links.
- Tauri folder picker and scoped native listing/TXT/Markdown reading commands.
- [Frozen cross-track contracts](contracts.md) declared in both `src/domain/contracts.ts` and `src-tauri/src/contracts.rs`: typed failures, stable workspace/document identity, UTF-8 source offsets bound to a document revision, typed relationship evidence, embedding-space fingerprints, provider error/cancellation codes, plans, approvals, per-operation outcomes, history and undo shapes.
- Native commands that return `{ code, message, details? }` instead of prose, a workspace registry that refuses an identity it never issued, listing that reports files it cannot identify instead of renaming them, and reads that return the document's content hash.
- A native plan registry that issues plan identities and digests, binds approval to the exact digest, and runs read-only preflight over the real filesystem. `apply_plan` refuses with `writerNotImplemented`; no native write exists.
- Deterministic batch-outcome rules (succeeded/failed/cancelled/notStarted), whole-batch Undo conflict preflight, and restart handling that drops approvals, in both languages.
- Cross-language golden fixtures in `fixtures/contracts/` that pin hashes, identities, path rules, offsets, fingerprints and canonical plan bytes for the TypeScript and Rust suites.
- Configuration guards asserting the webview capability grants no filesystem, shell, HTTP or dialog permission and that the CSP allows no remote origin.
- A SQLite migration contract.
- Fifteen English/Filipino/Taglish synthetic text documents and labelled benchmark cases.
- Product docs, glossary, ADRs, four-person plan, and CI definitions.

## Pending

SQLite connection, text-PDF extraction, shared-fact discovery, durable apply/undo/history, incremental indexing, native packaging, and real Model Lab results remain pending. Issue #4 carries the multilingual embedding/local-generation adapters and proposal-only interpretation path below; it does not replace #3 persistence or #5 native apply/undo.

The native writer is [issue #5](https://github.com/duckycodess/folio/issues/5). Until it lands, no file has ever been written by Folio. The cases that need it are listed as pending, not mocked: 16 `todo` cases in `src/domain/pending.test.ts` and three `#[ignore]` tests in `src-tauri/src/plan.rs`. Interface and contract tests are not filesystem apply/undo evidence.

No AI or save completion should be presented without the corresponding native/provider evidence. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Remote CI verification after conflict resolution

GitHub Actions run [37939253553](https://github.com/duckycodess/folio/actions/runs/37939253553) passed at `7265b05` (2026-10-09): frontend formatting/type checks/tests/build plus the Linux core suite, and native `cargo test --manifest-path src-tauri/Cargo.toml` on Windows and macOS. This supersedes earlier compile/test uncertainty for that revision only. No local WSL tests were resumed. CI did not run gated real-model acceptance, desktop interaction, packaging, or 8-GB measurements, and did not resolve the then-pending static contract/identity integration findings. PR #15 remains draft.

The subsequent C1–C5 integration fixes are split into focused commits on
`FOLIO-4`: native registry resolution, native document identity and hash
parity, `FolioError` conversion, the additive `GroundedResult` boundary, and
offset-helper contract tests. Formatting and static diff checks passed locally;
local tests, builds and inference remain intentionally stopped because WSL
memory pressure caused repeated restarts. GitHub Actions run
[37943613336](https://github.com/duckycodess/folio/actions/runs/37943613336) at
`4da77bc` completed successfully: frontend reported 9 Vitest files passed and
1 skipped with 89 tests passed and 16 todo; Linux core reported 46 passed, 0
failed and 2 ignored out of 48; macOS native reported 81 passed, 0 failed and
3 ignored out of 84; Windows native reported 85 passed, 0 failed and 3
ignored out of 88. These are hosted compile/test results only. TJ review is
still required for the additive contract proposals and grounded-summary
correctness; no real-model acceptance, desktop interaction, packaging, or
8-GB measurement is claimed here.

## Verification

### Contract freeze and safety baseline (2026-10-09, issue #2)

Checked on Linux (WSL2) with Node.js 24.15.0, npm 11.12.1 and cargo 1.95.0:

- `npm run format:check`: passed.
- `npm run check`: passed.
- `npm test`: 85 tests passed, 16 explicitly pending (`todo`), across identity, UTF-8 offsets, typed failures and unknown-code handling, keyword retrieval over the synthetic corpus, typed relationship evidence, plan preflight/digest/approval, batch outcomes, whole-batch undo and the cross-language fixtures.
- `npm run build`: passed; the production frontend compiles.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 75 passed, 3 ignored (the pending native-writer cases). Covers folder escape and symlink escape against temporary synthetic folders, the workspace gate, non-Unicode filenames, byte lengths, plan digest forgery, stale and fabricated approvals, rename collision with the existing file read back unchanged, case-insensitive target aliases, batch-failure and cancellation outcomes, undo conflicts, the capability/CSP guards, and the shared contract fixtures.

The native count differs by platform, because some fixtures can only exist on some systems. Symbolic links are exercised on Unix. A filename that is not valid UTF-8 can only be created where the filesystem stores arbitrary bytes — macOS refuses it at creation, and Windows filenames are UTF-16 — so the listing case that reports such a file as skipped runs only there; the refusal itself is covered on every platform by a filesystem-free test. A file whose name contains a backslash can only exist where backslash is not the path separator, so that identity case is Unix-only, while the separator-ambiguity refusal is checked everywhere.

`src-tauri/Cargo.lock` is committed so these runs are reproducible; it references only crates.io and contains no host paths or credentials. `fixtures/contracts/contract-cases.json` is reproduced byte for byte by `fixtures/contracts/generate-contract-cases.py`.

The Node checks were run from a disposable copy of this repository placed outside the home directory, and `cargo test` was run in place. Both ran on one Linux (WSL2) host; neither is evidence about any other platform. A pre-existing zero-byte `/home/tj/package.json` on this host breaks esbuild's config loader for every Vite project under that tree; it belongs to the host, not to Folio, and was left untouched. The same scripts run unchanged in CI.

### GitHub Actions on this branch

[Run 37934419707](https://github.com/duckycodess/folio/actions/runs/37934419707), for commit `5339690` on [PR #13](https://github.com/duckycodess/folio/pull/13): the frontend job passed; both `desktop-check` jobs failed. macOS could not create a filename that is not valid UTF-8 ("Illegal byte sequence"), and Windows could not create a file whose name contains a backslash, because there it is the path separator. Both were test fixtures assuming the host's filesystem, not defects in the code under test; the surrounding assertions passed on every platform. The fixtures are now scoped to the systems that can hold them, and the refusals they checked are covered everywhere by filesystem-free tests.

[Run 37935515689](https://github.com/duckycodess/folio/actions/runs/37935515689), for commit `fab1680`, passed all three jobs: frontend, `desktop-check (macos-latest)` with 74 native tests passed and 3 ignored, and `desktop-check (windows-latest)` with 70 passed and 3 ignored. The counts differ from the 75 on Linux exactly by the fixtures each platform cannot hold — macOS lacks the one needing a filename that is not valid UTF-8 on disk, and Windows also lacks the four needing symbolic links or a backslash in a filename, while gaining its own UTF-16 version of the non-Unicode refusal.

These are results for the native test suite only. No desktop startup, folder picker, installer, bundle or local inference has been exercised on Windows or macOS, and CI does not do so. Issue #2 stays open until that evidence exists.

Not verified by this work, and not claimed: desktop startup and the native folder picker on actual Windows and macOS, any native file write, undo against real files, installers or bundles, local inference, and installed size, memory or responsiveness budgets. Issue #2 stays open until real Windows and macOS startup and folder-picker evidence exists.

### Documentation and starter baseline (2026-10-09)

Checked with Node.js 24.19.0:

- `npm run check`: passed.
- `npm test`: 9 tests passed, covering approval/expiry/current-content checks, required history, explicit-reference containment, Filipino keyword retrieval, and embedding-space fingerprints.
- `npm run build`: passed; the production frontend compiles.
- SQLite migration: executed in memory with Python's SQLite; Filipino FTS matching, insert/update/delete triggers, embedding revision separation, foreign-key guards, and cascading cleanup passed.
- `npm run tauri info`: configuration recognized. Native tests were not run on the original Linux preparation host. [GitHub Checks run 37913994204](https://github.com/duckycodess/folio/actions/runs/37913994204), for commit `52208c6ded13c515ade8d438a27ec58ba0012931`, passed frontend checks and native `cargo test` on Windows and macOS. CI did not build/install consumer bundles or exercise local inference and full offline workflows.
- The original browser attempt could not start. A later visual/interaction review on 2026-10-09 used headless Windows Edge at 1366×768, 1024×768 and 390×844 against the browser fixtures. Search, Organize, Summarize and Ask & Act rendered without horizontal page overflow or JavaScript exceptions; Escape closed the assistant and restored focus. Readability, layout, hidden search evidence and cross-workflow notice defects remain tracked in [UI replacement #6](https://github.com/duckycodess/folio/issues/6) and [workflow UI #7](https://github.com/duckycodess/folio/issues/7). This did not verify native folder picking, inference, writes or installers.

Local inference, PDF extraction, filesystem apply/undo, native packaging, and actual performance/size measurements remain unverified and unimplemented as described above.

### Issue #4 carried-forward provider and proposal evidence

The FOLIO-4 branch retains the pure `folio-core` workspace member, verified model/runtime manifest, local E5 and llama.cpp adapters, grounded summaries/answers, relevance/no-generator gate, cancellation and lifecycle handling, and deterministic proposal-only interpretation. The incoming issue #2 native shell remains authoritative for workspace identity, typed errors, action-plan identity/digest, approval, and the refusal to write until issue #5. AI output is never an approval and retrieved document text never authorizes a filesystem operation.

Evidence from the pre-merge FOLIO-4 checkout is preserved as historical context,
not as verification for the merged branch: frontend checks previously passed
(11 tests and production build); the post-restart core suite passed 43 tests
with 0 failures and 2 ignored; the root Tauri test/build passed earlier at
`e5a9e5b` (5 app tests and debug build), while a later root rerun was
interrupted by WSL restart. The current branch's hosted verification is the
named run above; the ignored real-provider R8 harness has no new adapter
output, and summary correctness remains TJ Not reviewed.

The WSL native prerequisites were user-installed and verified at WebKitGTK/JavaScriptCoreGTK 2.52.6, libsoup 3.4.4, librsvg 2.58.0, with Cargo/rustc 1.96.1 available through the inline user-local PATH. Historical missing-library and Cargo 1.75 failures remain historical only. Windows Rust-native, macOS, desktop startup/folder picker, packaging, Job Object behavior, target RAM/size, #3 persistence, and #5 apply/undo remain unverified.

No local test, build, or inference was run after the merge resolution because
the user instructed that WSL-heavy execution remain stopped. Opus approved the
static C1–C5 integration review at `4da77bc`, and the named hosted Actions run
provides the current compile/test evidence.

## Product-context refresh

On 2026-10-09, the user confirmed the settled scope and authorized Impeccable init plus a direct main-branch push. The existing `docs/product.md` was updated in place with product-schema version 1, the webview platform classification, users, purpose, positioning, operating context, evidence, product principles and accessibility requirements. No competing root `PRODUCT.md` or replacement visual world was created.

The named team plan now gives TJ most checking and integration/release coordination, Dann local AI, Gab native workspace/actions and Louise the replacement UI. The full MVP and current UI replacement remain implementation work; this documentation refresh does not complete any of those features.

Documentation-refresh validation passed: Impeccable product schema 1 and the webview platform value, all 15 local Markdown references, preservation of the four original scope sections, named ownership and all ten issue links, and Prettier formatting for the three changed documentation files. Application and native tests were not rerun for this documentation-only refresh.
