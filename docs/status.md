# Implementation status

## Implemented starter pieces

- Golden Daylight app shell ([issue #16](https://github.com/duckycodess/folio/issues/16)): design tokens with light and proposed dark themes, locally bundled Inter and Lucide icons, sidebar navigation (Home, Files, Organize, Graph, Ask & Act, Model Lab), a global search field with a platform-aware ⌘K / Ctrl K shortcut, a document panel with Summary, Details and Related tabs, and shared button, badge, panel, list row, empty state, notice, modal and progress components. The starter `App.tsx` presentation and `src/styles.css` are retired. Summaries, Ask & Act, collections, renames and Model Lab show honest "not available yet" states; nothing is presented as AI output or a saved change.
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
- Golden Daylight design system and Olio mascot rules recorded in [design.md](design.md); not yet implemented in the UI. The dark theme and its contrast ratios are computed, not visually reviewed, and the Olio poses and wordmark SVG are still to be produced.

## Pending

Multilingual embedding integration, semantic search, SQLite connection, text-PDF extraction, model lifecycle/downloads, local generation, AI summaries, command interpretation, shared-fact discovery, durable apply/undo/history, incremental indexing, and real Model Lab results.

The native writer is [issue #5](https://github.com/duckycodess/folio/issues/5). Until it lands, no file has ever been written by Folio. The cases that need it are listed as pending, not mocked: 16 `todo` cases in `src/domain/pending.test.ts` and three `#[ignore]` tests in `src-tauri/src/plan.rs`. Interface and contract tests are not filesystem apply/undo evidence.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Verification

### App shell and design tokens (2026-10-09, issue #16)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 90 passed, 16 todo. The new cases cover the platform-aware search shortcut and the navigation keeping Organize separate from Ask & Act.
- Browser preview (sample fixtures only) in headless Google Chrome via Playwright at 1280×850 (light and dark), 1024×768, 700×800, and 640×425 at device scale 2 (200% zoom of 1280×850):
  - no horizontal page scroll;
  - Inter loaded from the bundle;
  - smallest rendered text 12px;
  - at 1280 and 1024 the file list and the selected document's panel are both on the first screen;
  - at 700px and 200% zoom the document replaces the list, with a Back control.
- Keyboard checks in the same run:
  - ⌘K focuses search on macOS, and Ctrl+K is ignored there;
  - arrow keys move between file rows and between document tabs;
  - the active nav item has `aria-current="page"`;
  - Escape closes the rename preview and returns focus to its trigger.

Not verified: Ctrl K on Windows (unit-tested only), screen readers, the Tauri webview, native folder picking, and the dark theme's visual review. Contrast follows the measured token pairs in `docs/design.md`; no automated contrast audit was run on rendered pages.

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

## Product-context refresh

On 2026-10-09, the user confirmed the settled scope and authorized Impeccable init plus a direct main-branch push. The existing `docs/product.md` was updated in place with product-schema version 1, the webview platform classification, users, purpose, positioning, operating context, evidence, product principles and accessibility requirements. No competing root `PRODUCT.md` or replacement visual world was created.

The named team plan now gives TJ most checking and integration/release coordination, Dann local AI, Gab native workspace/actions and Louise the replacement UI. The full MVP and current UI replacement remain implementation work; this documentation refresh does not complete any of those features.

Documentation-refresh validation passed: Impeccable product schema 1 and the webview platform value, all 15 local Markdown references, preservation of the four original scope sections, named ownership and all ten issue links, and Prettier formatting for the three changed documentation files. Application and native tests were not rerun for this documentation-only refresh.
