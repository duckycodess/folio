# Implementation status

## Implemented starter pieces

- SOS React interface with workflows A/B/C and synthetic document navigation.
- Keyword filtering (explicitly labelled), actual Markdown-link discovery in fixture text, source content views, and a graph of those explicit links.
- Tauri folder picker and scoped native listing/TXT/Markdown reading commands.
- [Frozen cross-track contracts](contracts.md) declared in both `src/domain/contracts.ts` and `src-tauri/src/contracts.rs`: typed failures, stable workspace/document identity, UTF-8 source offsets bound to a document revision, typed relationship evidence, embedding-space fingerprints, provider error/cancellation codes, plans, approvals, per-operation outcomes, history and undo shapes.
- Native commands that return `{ code, message, details? }` instead of prose, a workspace registry that refuses an identity it never issued, listing that reports files it cannot identify instead of renaming them, and reads that return the document's content hash.
- A native plan registry that issues plan identities and digests, binds approval to the exact digest, and runs read-only preflight over the real filesystem. Since issue #5, `apply_plan` runs the native writer (below).
- Deterministic batch-outcome rules (succeeded/failed/cancelled/notStarted), whole-batch Undo conflict preflight, and restart handling that drops approvals, in both languages.
- Cross-language golden fixtures in `fixtures/contracts/` that pin hashes, identities, path rules, offsets, fingerprints and canonical plan bytes for the TypeScript and Rust suites.
- Configuration guards asserting the webview capability grants no filesystem, shell, HTTP or dialog permission and that the CSP allows no remote origin.
- A SQLite migration contract.
- Fifteen English/Filipino/Taglish synthetic text documents and labelled benchmark cases.
- Product docs, glossary, ADRs, four-person plan, and CI definitions.

## Native workspace and index (issue #3)

- SQLite in the OS app-data directory (`folio.sqlite`), migrated through `PRAGMA user_version`; WAL mode, so scans run on their own connection.
- Folders chosen in the native picker are remembered and can be reopened by id; reopening is refused when the folder is gone, unreadable or resolves elsewhere (ADR 0007).
- Incremental scan of TXT, Markdown and text-based PDFs (per page, via `lopdf`). Size+mtime, then SHA-256, decide whether a file is re-read. Hidden folders, `node_modules` and symlinks are skipped. Progress events, cancellation between files, batches of 50.
- Scanned/image-only PDFs are `unsupported`; unreadable new files are `failed`; a changed file that cannot be re-read keeps its previous chunks and is marked `stale`. `stale` and `failed` documents are re-read on every scan, so a file that was locked, offline or briefly unreadable recovers once it can be read.
- A folder or file that cannot be read during a scan is not treated as deleted: records at or below it keep their data and status. Only files that are really gone are removed.
- Each batch reads, hashes and extracts its files before opening the write transaction, so other writes (remembering a folder, storing vectors) are not blocked behind PDF extraction.
- PDF object, cross-reference and page streams are each bounded at 16 MiB of decompressed data while loading and extracting, so a decompression bomb is dropped rather than allocated. A page whose text cannot be extracted is skipped and named in the document's status message; a PDF whose pages all fail is `failed`, not reported as a scan.
- Changed or deleted documents drop their chunks, vectors, relationships and cached summaries. Markdown-link relationships are rebuilt with located link evidence.
- FTS5 keyword search across unopened documents, returning document id, path and excerpt as `utf8Byte` passages bound to the indexed revision (`documentContentHash`), with page numbers for PDFs. Labelled `keyword`.
- Follows the [frozen contract](contracts.md): `workspaceId:relativePath` document ids, `sha256:` hashes, `explicitReference` relationships with the raw and resolved link, `folio-space-v1/...` space fingerprints, and `{ code, message, details }` failures. `read_document` now also returns the extracted text of text-based PDFs.
- Exact-duplicate groups are confirmed by comparing the files byte for byte in blocks, so files of any size are verified; the comparison runs without holding the index.
- Embedding store for the provider track: registered spaces keyed by model/revision/quantization/dimensions/preprocessing, per-space pending-chunk listing, vector storage with dimension checks, and exact cosine search within one space only. A vector is stored only while its chunk still holds the text it was computed from (`contentHash`), because chunk ids can be reused after a rescan. No embedding model is connected.

## Native writer, Ripple, history and Undo (issue #5)

- `apply_plan` runs the native writer (`src-tauri/src/writer.rs`) on a plan the registry issued and the user approved, after re-checking approval, digest, expiry and every target. Each completed operation gets a durable outcome and a history entry in SQLite; the first failure stops the batch and keeps earlier changes for Undo; a cancellation lets the running operation finish. Affected index entries are refreshed after the writes. A plan is retired once applied, and a recorded plan cannot be applied twice. The plan's record and its approval are stored in one transaction before the first write, so a failed setup leaves nothing that looks applied.
- An error from `apply_plan` means no file changed. Once an operation has run, the report is always returned; bookkeeping that fails afterwards is reported as `historySettled: false`, not as a failure of the writes.
- Apply and Undo run off the async workers on their own index connection, waiting for any running scan first. While a scan runs, Apply waits silently; there is no "waiting for indexing" signal for the UI yet.
- Renames and moves never replace an existing file (hard link, then unlink; a checked rename on filesystems without hard links). Creates use exclusive creation. Every operation re-checks its source's hash just before it runs, so a file edited after preflight is kept and the operation fails with `targetChanged`. Edits write a hidden temporary file and check the hash again immediately before swapping it in.
- Edits and Undo keep the file's permissions (the Unix mode, or the Windows read-only attribute). A file Folio may not write, read-only or owned by someone else, is refused instead of replaced. Ownership, ACLs and extended attributes are not carried over; whether explicit Windows ACLs survive has not been checked.
- A scan removes the writer's own temporary files (`.<name>.folio-<pid>-<n>.tmp`) older than 15 minutes, which only an interrupted save leaves behind. No other file is removed.
- Undo is preview-then-confirm (ADR 0008): `preview_undo` returns the preflight, and `undo_plan` reverses only the exact entries the user confirmed, newest first. Conflicts change nothing; a partial Undo leaves the rest pending for a fresh preview.
- History listing is bounded (default 100, at most 500 entries, one query). Edits keep previous content for the 100 most recent plans; renames, moves and creates need none and stay undoable. Stored plans keep a summary and the Ripple evidence, never file bodies.
- Ripple (`ripple.rs`): documents linked to or from the target, or shared-fact candidates, that mention the replaced phrase are `evidence`; similarity relationships and byte-identical copies are `similarityOnly`; unrelated documents sharing the value are omitted. Whole-phrase matching includes English, abbreviated and Filipino month names. For a date in May, only a capitalised "May" counts, because lowercase Filipino _may_ means "there is"; a sentence that begins "May 20 …" still reads as the month. Candidates carry `relationshipType`/`provenance` and are capped at 25. The phrase comes from the edit's diff, or from `ripple_impacts` when a caller knows it.
- `prepare_passage_edit` builds the frozen whole-file edit from an exact passage that occurs once; Organization Suggestions return verified duplicate groups and title-based filenames with their rename operations.

## Pending

Multilingual embedding integration, semantic search, model lifecycle/downloads, local generation, AI summaries, command interpretation, model-generated Ripple explanations and similarity/shared-fact discovery (issues #4 and #8), creating folders during moves, UI use of the native index and actions (the current UI still searches loaded content), live file watching, multi-folder workspaces, and real Model Lab results.

Provider cases are listed as pending, not mocked, in `src/domain/pending.test.ts`. Writer tests use real temporary folders; they are not evidence about the desktop window, installers or a real user's folders.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

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

### Native workspace and index (issue #3, 2026-10-09)

Checked on Windows 11 (x64) with Rust 1.91.1 and Node.js 20.20.2 (below the 22.12 engines baseline; CI uses 22), after merging the frozen contract from `main`:

- `cargo test --manifest-path src-tauri/Cargo.toml`: 112 passed, 4 ignored (TJ's three pending native-writer cases and the PDF-fixture generator), including TJ's review repros for reused chunk ids and their cross-platform counterparts for stale retries and unreadable folders (the Unix `chmod` versions run in CI on macOS), a decompression bomb, PDF pages that cannot be extracted, accent-folded excerpts, duplicates above 20 MiB and the 5,000-document limit. The index tests cover migrations/FTS5, UTF-8 byte offsets that slice the file bytes on non-ASCII text and name its revision, PDF per-page extraction, scanned and corrupt PDFs, an unchanged second scan, external edit/delete invalidation (chunks, relationships, caches, vectors), stale-on-failure, hidden/dependency/symlink exclusion, an escaping symlink read (it actually ran on this Windows host), `..`/absolute paths, lost folders, cancellation, progress phases, link evidence (including the copy's broken link), byte-verified duplicates, restart persistence and reopen, and embedding-space isolation.
- `npm run check`, `npm test` (85 passed, 16 pending `todo`), `npm run build`: passed after the additive index types and adapter functions.
- `npm run tauri dev` on Windows: the app boots and creates `folio.sqlite` (plus WAL files) in `%APPDATA%\dev.folio.desktop`. The folder picker, scan and search were not exercised through the real window, since that needs a person at the dialog.
- Not run: any macOS native test. macOS relies on the CI `desktop-check` job.

### Native writer, Ripple and Undo (issue #5, 2026-10-09)

Checked on the same Windows host after merging the frozen contract:

- `cargo test --manifest-path src-tauri/Cargo.toml`: 130 passed, 1 ignored (the PDF-fixture generator), including the index fixes from TJ's review of PR #11. The writer tests run against temporary copies of the fixture corpus and cover:
  - the October 20 → 23 Ripple case: three linked notes as evidence, the copy as similarity-only, no math files, and only the plan file changed by hash;
  - Filipino month names, a shared-fact candidate as evidence, and "October 2026" not matching;
  - real create, rename and move with history;
  - injected failures on a second edit, rename, move and create: earlier changes are kept and then reversed through Undo;
  - cancellation;
  - a destination or external edit that appears after preflight, kept untouched;
  - a read-only target (Windows) or folder (Unix), and a folder that disappeared;
  - Undo refused for an unconfirmed or stale preview and after an external edit, and finished after a partial stop;
  - history across a restart, pruning, bounded listing, passage edits, and stored plans without file bodies.
- `npm run check`, `npm test` (85 passed, 9 pending provider/platform `todo`), `npm run build`: passed.
- Not exercised: the real desktop window driving apply/Undo, any macOS run (CI only), installers, and local inference.

### TJ's review of PR #12 (2026-10-09)

Fixes for the review: edits keep permissions and refuse files Folio may not write, the apply setup is one transaction, an apply that changed files always returns its report (`historySettled`), renames and moves re-check their source, abandoned temporary files are cleaned up, apply and Undo run off the async workers, Filipino _may_ no longer matches "May 20", and the stale writer placeholder text in `plan.rs` is gone. New tests cover each of these, including TJ's two permission repros on Unix (`0600` kept through an edit and its Undo, and a `0444` file refused).

These changes were written on a Linux host without a Rust toolchain or the Tauri system libraries, so `cargo test` was not run locally; the native suite is verified by the CI `desktop-check` jobs only. `npm run check`, `npm test` and `npm run build` were run locally.

Local inference, native packaging, and actual performance/size measurements remain unverified. Indexing time and database size have not been measured.

## Product-context refresh

On 2026-10-09, the user confirmed the settled scope and authorized Impeccable init plus a direct main-branch push. The existing `docs/product.md` was updated in place with product-schema version 1, the webview platform classification, users, purpose, positioning, operating context, evidence, product principles and accessibility requirements. No competing root `PRODUCT.md` or replacement visual world was created.

The named team plan now gives TJ most checking and integration/release coordination, Dann local AI, Gab native workspace/actions and Louise the replacement UI. The full MVP and current UI replacement remain implementation work; this documentation refresh does not complete any of those features.

Documentation-refresh validation passed: Impeccable product schema 1 and the webview platform value, all 15 local Markdown references, preservation of the four original scope sections, named ownership and all ten issue links, and Prettier formatting for the three changed documentation files. Application and native tests were not rerun for this documentation-only refresh.
