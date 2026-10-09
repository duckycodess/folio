# Implementation status

## Implemented starter pieces

- SOS React interface with workflows A/B/C and synthetic document navigation.
- Keyword filtering (explicitly labelled), actual Markdown-link discovery in fixture text, source content views, and a graph of those explicit links.
- Tauri folder picker and scoped native listing/TXT/Markdown reading commands.
- Shared document, evidence, relationship, action-plan, provider, and benchmark contracts.
- Deterministic approval-state logic and a SQLite migration contract.
- Fifteen English/Filipino/Taglish synthetic text documents and labelled benchmark cases.
- Product docs, glossary, ADRs, four-person plan, and CI definitions.

## Native workspace and index (issue #3)

- SQLite in the OS app-data directory (`folio.sqlite`), migrated through `PRAGMA user_version`; WAL mode, so scans run on their own connection.
- Folders chosen in the native picker are remembered and can be reopened by id; reopening is refused when the folder is gone, unreadable or resolves elsewhere (ADR 0005).
- Incremental scan of TXT, Markdown and text-based PDFs (per page, via `lopdf`). Size+mtime, then SHA-256, decide whether a file is re-read. Hidden folders, `node_modules` and symlinks are skipped. Progress events, cancellation between files, batches of 50.
- Scanned/image-only PDFs are `unsupported`; unreadable new files are `failed`; a changed file that cannot be re-read keeps its previous chunks and is marked `stale`.
- Changed or deleted documents drop their chunks, vectors, relationships and cached summaries. Markdown-link relationships are rebuilt with located link evidence.
- FTS5 keyword search across unopened documents, returning document id, path, excerpt and UTF-16 offsets/page. Labelled `keyword`.
- Exact-duplicate groups are confirmed by re-reading and comparing bytes.
- Embedding store for the provider track: registered spaces keyed by model/revision/quantization/dimensions/preprocessing, per-space pending-chunk listing, vector storage with dimension checks, and exact cosine search within one space only. No embedding model is connected.

## Native actions, Ripple, history and undo (issue #5)

- Native plans bound to workspace, document ids, exact paths and expected SHA-256 hashes; 10-minute expiry; SHA-256 digest that approval must echo. Previews write nothing.
- Operations: TXT/Markdown passage edit (the passage must occur exactly once), create, rename (same folder), move (existing folder). PDF content edits, path/symlink escapes, collisions, missing folders, stale index entries and unknown document ids are refused.
- Apply re-checks every target and destination, writes via a hidden temp file + rename, records history (previous content kept for the last 100 plans), then refreshes the affected index entries and links. A failed write reverts earlier writes where the file is still what Folio wrote and reports what was restored; the index is not updated.
- Undo reverses a whole plan after verifying every file still matches what Folio wrote; otherwise `UNDO_CONFLICT` and nothing changes.
- Ripple: documents linked to/from the target that mention the replaced value are evidence; byte-identical copies are similarity-only; unrelated documents that share the value are omitted. No generated explanations yet (provider track).
- Organize: verified duplicate groups (evidence only) and title-based filename suggestions that become rename plans.

## Pending

Multilingual embedding integration, semantic search, model lifecycle/downloads, local generation, AI summaries, command interpretation, model-generated Ripple explanations and similarity candidates, shared-fact discovery, creating folders during moves, UI use of the native index and action commands (the current UI still searches loaded content and only previews a string), live file watching, multi-folder workspaces, and real Model Lab results.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Verification

Checked on 2026-10-09 with Node.js 24.19.0:

- `npm run check`: passed.
- `npm test`: 9 tests passed, covering approval/expiry/current-content checks, required history, explicit-reference containment, Filipino keyword retrieval, and embedding-space fingerprints.
- `npm run build`: passed; the production frontend compiles.
- SQLite migration: executed in memory with Python's SQLite; Filipino FTS matching, insert/update/delete triggers, embedding revision separation, foreign-key guards, and cascading cleanup passed.
- `npm run tauri info`: configuration recognized. Native tests were not run on the original Linux preparation host. [GitHub Checks run 37913994204](https://github.com/duckycodess/folio/actions/runs/37913994204), for commit `52208c6ded13c515ade8d438a27ec58ba0012931`, passed frontend checks and native `cargo test` on Windows and macOS. CI did not build/install consumer bundles or exercise local inference and full offline workflows.
- The original browser attempt could not start. A later visual/interaction review on 2026-10-09 used headless Windows Edge at 1366×768, 1024×768 and 390×844 against the browser fixtures. Search, Organize, Summarize and Ask & Act rendered without horizontal page overflow or JavaScript exceptions; Escape closed the assistant and restored focus. Readability, layout, hidden search evidence and cross-workflow notice defects remain tracked in [UI replacement #6](https://github.com/duckycodess/folio/issues/6) and [workflow UI #7](https://github.com/duckycodess/folio/issues/7). This did not verify native folder picking, inference, writes or installers.

Checked on 2026-10-09 on Windows 11 (x64) with Rust 1.91.1 and Node.js 20.20.2 (below the 22.12 engines baseline; CI uses 22):

- `cargo test --manifest-path src-tauri/Cargo.toml`: 51 passed, 1 ignored (the PDF-fixture generator). Issue #3 coverage: Covers migrations/FTS5, UTF-16 chunk and excerpt offsets on non-ASCII text, PDF per-page extraction, scanned and corrupt PDFs, an unchanged second scan, external edit/delete invalidation (chunks, relationships, caches, vectors), stale-on-failure, hidden/dependency/symlink exclusion, an escaping symlink read (it actually ran on this Windows host), `..`/absolute paths, lost folders, cancellation, progress phases, link evidence (including the copy's broken link), byte-verified duplicates, restart persistence and reopen, and embedding-space isolation.
  Issue #5 coverage, with real writes against temporary fixture copies (which have CRLF line endings on this host):
  - Ripple deadline case: the three linked notes are flagged, the copy is similarity-only, the math files are absent, and only `projects/project-plan.md` changes on disk.
  - Refusals with no file changes: apply without approval, expired plans, a UI-invented digest, a tampered stored plan, an external edit after approval, rename collision, `..`/absolute/symlinked-folder destinations, a missing folder, a PDF edit, ambiguous or missing passages, path-as-id, and a stale index.
  - Create, rename and move change real files and record history; the renamed document keeps its id.
  - Injected failure on the second write reverts the first and leaves the index unchanged.
  - Undo restores bytes and paths and refuses after external edits; a create's undo refuses if the new file was changed; pruned history is listed but can't be undone.
- `npm run check`, `npm test` (9 passed), `npm run build`: passed after the adapter/contract additions.
- `npm run tauri dev` on Windows: the app boots and creates `folio.sqlite` (plus WAL files) in `%APPDATA%\dev.folio.desktop`. The folder picker, scan and search were not exercised through the real window, since that needs a person at the dialog.
- CI run 37932669223 (PR #12, which contains the #3 and #5 changes): `frontend`, `desktop-check (windows-latest)` and `desktop-check (macos-latest)` passed, so `cargo test` ran on macOS in CI. No local macOS run, installer, or real-window workflow was tested.

Local inference, filesystem apply/undo, native packaging, and actual performance/size measurements remain unverified and unimplemented as described above. Indexing time and database size have not been measured.

## Product-context refresh

On 2026-10-09, the user confirmed the settled scope and authorized Impeccable init plus a direct main-branch push. The existing `docs/product.md` was updated in place with product-schema version 1, the webview platform classification, users, purpose, positioning, operating context, evidence, product principles and accessibility requirements. No competing root `PRODUCT.md` or replacement visual world was created.

The named team plan now gives TJ most checking and integration/release coordination, Dann local AI, Gab native workspace/actions and Louise the replacement UI. The full MVP and current UI replacement remain implementation work; this documentation refresh does not complete any of those features.

Documentation-refresh validation passed: Impeccable product schema 1 and the webview platform value, all 15 local Markdown references, preservation of the four original scope sections, named ownership and all ten issue links, and Prettier formatting for the three changed documentation files. Application and native tests were not rerun for this documentation-only refresh.
