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

## Pending

Multilingual embedding integration, semantic search, model lifecycle/downloads, local generation, AI summaries, command interpretation, shared-fact discovery, durable apply/undo/history, UI use of the native index (the current UI still searches loaded content), live file watching, multi-folder workspaces, and real Model Lab results.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Verification

Checked on 2026-10-09 with Node.js 24.19.0:

- `npm run check`: passed.
- `npm test`: 9 tests passed, covering approval/expiry/current-content checks, required history, explicit-reference containment, Filipino keyword retrieval, and embedding-space fingerprints.
- `npm run build`: passed; the production frontend compiles.
- SQLite migration: executed in memory with Python's SQLite; Filipino FTS matching, insert/update/delete triggers, embedding revision separation, foreign-key guards, and cascading cleanup passed.
- `npm run tauri info`: configuration recognized. Native builds/tests were not run because Rust/Cargo and Linux desktop libraries are unavailable here. Windows/macOS CI jobs are configured but have not run yet.
- Browser smoke checks were attempted but could not start: the browser binary is absent and its download returned an invalid archive. Visual behavior and native folder-picker behavior still require verification.

Checked on 2026-10-09 on Windows 11 (x64) with Rust 1.91.1 and Node.js 20.20.2 (below the 22.12 engines baseline; CI uses 22):

- `cargo test --manifest-path src-tauri/Cargo.toml`: 34 passed, 1 ignored (the PDF-fixture generator). Covers migrations/FTS5, UTF-16 chunk and excerpt offsets on non-ASCII text, PDF per-page extraction, scanned and corrupt PDFs, an unchanged second scan, external edit/delete invalidation (chunks, relationships, caches, vectors), stale-on-failure, hidden/dependency/symlink exclusion, an escaping symlink read (it actually ran on this Windows host), `..`/absolute paths, lost folders, cancellation, progress phases, link evidence (including the copy's broken link), byte-verified duplicates, restart persistence and reopen, and embedding-space isolation.
- `npm run check`, `npm test` (9 passed), `npm run build`: passed after the adapter/contract additions.
- `npm run tauri dev` on Windows: the app boots and creates `folio.sqlite` (plus WAL files) in `%APPDATA%\dev.folio.desktop`. The folder picker, scan and search were not exercised through the real window, since that needs a person at the dialog.
- Not run: any macOS native test. macOS relies on the CI `desktop-check` job.

Local inference, filesystem apply/undo, native packaging, and actual performance/size measurements remain unverified and unimplemented as described above. Indexing time and database size have not been measured.
