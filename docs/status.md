# Implementation status

## Implemented starter pieces

- Golden Daylight app shell ([issue #16](https://github.com/duckycodess/folio/issues/16)): design tokens with light and proposed dark themes, locally bundled Inter and Lucide icons, sidebar navigation (Home, Files, Organize, Graph, Ask & Act, Model Lab), a global search field with a platform-aware ⌘K / Ctrl K shortcut, a document panel with Summary, Details and Related tabs, and shared button, badge, panel, list row, empty state, notice, modal and progress components. The starter `App.tsx` presentation and `src/styles.css` are retired. Summaries, Ask & Act, collections, renames and Model Lab show honest "not available yet" states; nothing is presented as AI output or a saved change.
- Olio mascot artwork: twelve cleaned poses bundled in `src/assets/olio/`. Home's header pose follows the file list (default, confused for no results, peeking for an empty folder). The Files empty states, Organize's empty Collections and Model Lab's "no model" state also show a pose. The wordmark is still interim text.
- A sidebar theme switch (System, Light, Dark), remembered on the device, and coloured file-type tiles in file lists and the document panel. Checked in headless Chromium: the switch cycles, the choice survives a reload, and System removes the override. The sample files are all Markdown, so the PDF and text tiles have not been seen rendered.
- File table and reader ([issue #17](https://github.com/duckycodess/folio/issues/17)): name, location, type, modified and size columns that drop to fit the space; the reader beside the table (or in place of it below 860px) shows the file's text as read-only, with its full path. The reader never shows a file the current list excludes. In the desktop app, Folio starts with an "Add folder" state (sample files on request); an added folder with no readable files offers "Choose another folder". The browser preview lists sample files and says so.
- Relationships with evidence ([issue #21](https://github.com/duckycodess/folio/issues/21)): the document panel's Related tab and the Graph view list every connected file. Each entry has its type (link and direction, or exact duplicate), how Folio knows it, the file's original folder, and evidence excerpts that open the source with the passage highlighted. Files opened from Related keep a "Back to" link to the origin. With a folder open, links and duplicates come from the native index (`list_relationships`, `list_duplicates`) merged with links in opened files. If the folder hasn't been indexed, the UI says so; no UI runs the index scan yet. Exact duplicates are also found from content hashes Folio already has. The list is the only Graph view; there is no drawn graph. Similarity and shared-fact connections have labels and tests, but no producer yet.
- Shared error and recovery states ([issue #18](https://github.com/duckycodess/folio/issues/18)): every error code maps to one plain-language message and next step ([error-states.md](error-states.md)), shown through one recovery notice in every workflow. Drafts (the Ask & Act request, rename names per file) survive errors and view changes. Success after opening a folder is shown only once the native core reports it. Each view announces into its own live region. Modals keep Tab inside them. A browser-only practice mode (`?simulate=<code>`) triggers each state in its own flow.
- Home as the file browser ([#42](https://github.com/duckycodess/folio/issues/42), [#43](https://github.com/duckycodess/folio/issues/43), ADR 0010). The Files tab is gone. Home lists every file, sorted by path, with a count. Each row has a keyboard-accessible ⋯ menu (Open, Rename…, Move to folder…, Show related), and the document panel has the same menu. Rename and Move use the exact preview, Approve and Undo flow from #22, in a dialog. The search field is only on Home, centred, and ⌘K / Ctrl K from any page opens Home and focuses it. Rows take an optional `renderDetail` slot for #19's search evidence. With the sample files, Rename and Move explain that a folder is needed.
- Organize flow ([issue #22](https://github.com/duckycodess/folio/issues/22)), desktop only. Analyze re-indexes the open folder, with live progress and Stop, then lists exact duplicates (by content, never moved or deleted) and filename suggestions to tick. The exact preview shows every from → to path from the native plan. Approve echoes that plan's digest and applies it. The result is worded from the per-file outcomes, so a batch that stopped partway never says nothing changed. It shows what was recorded in history and offers a Preview Undo. A refused apply keeps the preview, with Preview again. The Rename form uses the same native plan with a folder open. With the sample files, Organize explains that a folder is needed. Virtual collections are still not available.
- Search evidence ([issue #19](https://github.com/duckycodess/folio/issues/19)): each Home search result shows how it matched (words in the text or in the name; "Similar meaning" only for semantic results) and up to two excerpts with the query words highlighted, case- and accent-insensitive, plus page labels for PDFs. Selecting an excerpt opens the reader at that highlighted passage. In an open folder, text search uses the persistent index (FTS5 keyword search), merged with file-name matches. An unindexed folder says that only names are searched and offers **Index this folder**, with progress and Stop. A file kept open outside the results is labelled, and a note says that finding files by meaning needs a local AI model.
- Home filters, pinned folders and recent files ([issue #33](https://github.com/duckycodess/folio/issues/33)):
  - Folder, File type and Modified filters under the search field combine with the query. A folder includes its subfolders, and files with no recorded time pass only "Any time". The panel shows "N of M files", and an empty result offers Clear filters.
  - The reader follows the filters as it follows search: a file the filters hide closes the reader, and a labelled note offers Clear filters.
  - Pinned folders (pin from the Folder filter) are one-tap filters. Recent files lists files opened in Folio on this device. Both are remembered per folder on this device only; a pin is not a permission.
  - Filters are kept when leaving Home and coming back.
  - The empty Collections placeholder gives way once pins or recent files exist, so the file list stays on the first screen.
  - The heading stays "Your workspace", per #43.
- First-run onboarding ([issue #14](https://github.com/duckycodess/folio/issues/14), partly): in the desktop app, five skippable steps, shown until completed or skipped and reopened from the sidebar's "Setup guide".
  1. Welcome.
  2. Choose a folder: nothing is read before the system picker returns one, and a cancelled picker changes nothing.
  3. Local AI: explains what it's for and that Model Lab shows sizes before any download. Nothing downloads here.
  4. Index: real phases, Stop, and "Continue to Home while indexing".
  5. What Folio found: exact duplicates and links between files from the indexed folder, with paths and the linking text, or an honest empty state with Search, Organize and Ask & Act.
  - The model recommendation (device RAM, disk, exact size and revision) is not built yet; it waits on #24 (PR #52).
- Keyword filtering (explicitly labelled), actual Markdown-link discovery in fixture text, source content views, and a list of those explicit links with their evidence (Graph view).
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
- Golden Daylight design system and Olio mascot rules recorded in [design.md](design.md); not yet implemented in the UI. The dark theme and its contrast ratios are computed, not visually reviewed, and the Olio poses and wordmark SVG are still to be produced.

## Native workspace and index (issue #3)

- SQLite in the OS app-data directory (`folio.sqlite`), migrated through `PRAGMA user_version`; WAL mode, so scans run on their own connection.
- Folders chosen in the native picker are remembered and can be reopened by id; reopening is refused when the folder is gone, unreadable or resolves elsewhere (ADR 0007).
- Incremental scan of TXT, Markdown and text-based PDFs (per page, via `lopdf`). Size+mtime, then SHA-256, decide whether a file is re-read. Hidden folders, `node_modules` and symlinks are skipped. Progress events, cancellation between files, batches of 50.
- Scanned/image-only PDFs are `unsupported`; unreadable new files are `failed`; a changed file that cannot be re-read keeps its previous chunks and is marked `stale`. `stale` and `failed` documents are re-read even when their size and time match, so a file that was locked, offline or briefly unreadable recovers once it can be read. Documents that keep failing the same way back off (issue #28, ADR 0009). After two free retries, Folio waits 10 minutes, then 20, 40 and so on, up to 6 hours. It retries sooner when the file's change signature changes (size, modification time, plus change time and mode on Unix or attributes on Windows), when the extractor version changes, or when the user asks: the `recheckUnreadable` scan option, or `recheck_documents` for "Check again". A deferred document is still counted in `failed` or `stale`, and also in the scan summary's `deferred`. Its `retryAfterMs` says when Folio will next read it. Status messages say Folio will check again later, and read errors are worded for people; the operating system's text stays in `details.cause`. No UI calls the re-check yet.
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

### Onboarding (2026-10-10, issue #14)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 183 passed, 9 todo. The new cases:
  - onboarding starts once, in the desktop app only;
  - step order doesn't run off either end;
  - findings put duplicates first, then cross-folder links;
  - files the folder no longer lists are ignored, and nothing is invented;
  - the limit is respected.
- Headless Chrome, with the desktop commands stood in for by a browser mock:
  - the browser preview never shows onboarding;
  - in desktop mode each step's heading takes focus, and no native command runs before the folder is picked;
  - a cancelled picker leaves Continue disabled;
  - the chosen folder and its file count show, and the AI step offers no download;
  - indexing shows its phase with Stop and "Continue to Home while indexing";
  - "What Folio found" lists the folder's link with both paths and the linking text;
  - finishing opens Home, the Setup guide reopens it, and it doesn't return after a reload;
  - no horizontal scroll at 700px.

Not verified: the real picker and index in the desktop app, offline use after setup, model setup (waits on #24), screen readers, and the Tauri webview.

### Home filters, pins and recent files (2026-10-10, issue #33)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 178 passed, 9 todo. The new cases cover:
  - folder filters including subfolders but not same-prefix folders, and the top level;
  - type filters;
  - date filters that never guess a missing time;
  - combined filters;
  - the folder list;
  - recent-file order and limit;
  - pin toggling.
- Headless Chrome, with the desktop commands stood in for by a browser mock:
  - folder "research" shows 1 of 4 files, and adding PDF shows "No files match these filters" with Clear filters;
  - "Past week" and the pinned-folder chip filter correctly;
  - recent files appear newest first, labelled "Opened in Folio on this device";
  - a filter that hides the open file closes the reader and labels it;
  - filters survive a trip to Graph, and pins and recent files survive a reload;
  - the first file row is on the first screen at 1280×850 and at 1024×768 with the reader open;
  - no horizontal scroll at 700px.
- The filter selects first wrapped their labels around the controls, which gave them names like "FolderAll foldersTop…". The labels now point at the selects with `for`.

Not verified: a real folder through the desktop app, screen readers, and the Tauri webview.

### Search evidence (2026-10-10, issue #19)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 170 passed, 9 todo. The new cases:
  - index text matches come before name-only matches, without repeats, and a result for a file no longer listed is dropped;
  - match labels never call keyword matching semantic;
  - highlighting marks whole words, case- and accent-insensitive ("nino" marks "Niño"), and keeps the excerpt's exact text.
- Browser preview, sample files, headless Chrome at 1280×850:
  - "deadline" shows four results, each with "Words in the text" and a highlighted excerpt;
  - the row's spoken description includes the label and the excerpt;
  - the live region says "4 files match your search.";
  - selecting an excerpt opens the reader with that passage highlighted and focused;
  - an open file the search leaves out is labelled with "Clear search";
  - a query with no matches shows "No matching files".
- Folder mode, with the desktop app's commands stood in for by a browser mock (`choose_workspace`, `list_documents`, `list_indexed_documents`, `search_index`, `scan_workspace` with progress events, `read_document`):
  - before indexing, only names are searched, and the "Index this folder" note appears;
  - indexing shows its phase and progress;
  - afterwards, "panayam" lists both files that contain it, with highlights;
  - an excerpt opens the read file with the passage highlighted;
  - a PDF result shows "Page 3";
  - there's no horizontal scroll at 700px.

Not verified: the real native index (this used a mock), semantic or hybrid results (there's no embedding model yet), opening a PDF at its page (the reader can't show PDF text yet; see #47), cross-language matches (which need semantic search), screen readers, and the Tauri webview.

### Home as the file browser (2026-10-10, issues #42 and #43)

Checked on macOS with Node.js 26.10.0, on top of #22:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 158 passed, 9 todo. New cases cover the folders a file can move to (only inside the open folder), rename and move operations, and refused names (`/`, `\`, `..`, empty, unchanged).
- Browser preview (sample files) in headless Chromium:
  - the sidebar has no Files;
  - the top bar has no search field;
  - Home's field is centred (equal 168px gaps, 640px wide);
  - all 15 files are listed with a "15 files" badge;
  - ⌘K from Graph opens Home with the field focused;
  - the query filters Home and is kept after visiting Organize, which has no search field and no Rename panel;
  - Tab moves from a row to its ⋯ menu, and Enter opens Open, Rename…, Move to folder…, Show related;
  - Rename with the sample files says nothing was changed;
  - closing returns focus to the ⋯ button;
  - Show related opens the Related tab;
  - Escape in a menu doesn't close the reader;
  - at 700px with a document open, the search field stays visible and nothing scrolls sideways.
- With the **mocked** native core:
  - Rename from a row pre-selects the name without its extension, previews `school/copy of plan.md → school/renamed.md`, applies, and lists the folder again;
  - Move offers only folders inside the open folder and previews `school/copy of plan.md → copy of plan.md`.

Not verified: the real native core in the Tauri app, screen readers, and Windows.

### Organize flow (2026-10-09, issue #22)

Checked on macOS with Node.js 26.10.0, on top of #31:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 139 passed, 9 todo. New cases cover:
  - the flow's steps;
  - late replies being ignored;
  - a refused apply keeping the preview;
  - no apply without a native plan;
  - preview rows for every from → to;
  - "saved" only when every change succeeded and the index caught up;
  - a stopped batch never claiming nothing changed;
  - a cancelled batch reported by what it kept;
  - partial and blocked Undo wording.
- The real UI in headless Chromium, with a **mocked** native core injected as `window.__TAURI_INTERNALS__` (a test harness, not part of the app):
  - Analyze shows progress, then 2 suggestions and 1 duplicate group;
  - the preview lists both renames;
  - the approval echoes the shown plan's id and digest before apply;
  - the folder is listed again afterwards;
  - Undo reports "Undid 2 changes.";
  - a mid-batch failure reads "Saved 1 of 2 changes. Stopped at …" with the earlier change kept;
  - a refused apply keeps the preview and Preview again issues a new plan;
  - Stop sends `cancel_indexing`;
  - a manual rename reads the file before preparing its plan;
  - no horizontal scroll at 700px.

Not verified: the real native core in the Tauri app (all of the above used the mock), screen readers, and Windows.

### Error and recovery states (2026-10-09, issue #18)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 120 passed, 16 todo. The new cases:
  - every error code has a title, a message and a flow;
  - the wording contains no developer terms or code names;
  - refused changes say no file was changed;
  - model failures say the request is kept;
  - a code this build doesn't know falls back to the general message;
  - action state: success only from the native success of the pending request, never before; late replies to older requests are ignored, and a failure can't be overwritten;
  - practice mode is never active in the desktop app.
- Browser preview in headless Google Chrome via Playwright at 1280×850, once per code with `?simulate=<code>` (all 33):
  - each message appears in its flow with the expected next-step button;
  - danger messages are `role="alert"`, and the others reach the view's live region;
  - the typed rename name and the Ask & Act request are still there afterwards, including after Open Model Lab and back;
  - no rename preview opens on a refused change.
- Without practice mode:
  - there's no banner, and Add folder stays disabled in the browser;
  - an Ask & Act message is announced in the Ask & Act region, and that region is gone after switching to Organize;
  - in the rename preview, 8 Tabs and 8 Shift+Tabs stay inside the dialog, and Escape closes it and returns focus to Preview rename;
  - the rendered text of all six views contains none of "Track T…", "docs/", "engine", "adapter", "fixture", "payload", "null" or "undefined".

After #32 merged: `useWorkspace` no longer exposes the unused `neighbors`, and the Related and Graph views show a folder-index read failure through the shared recovery notice, with Try again re-reading the index. This was checked by `npm run check` and `npm test` (140 passed, 9 todo). It wasn't rendered, because it needs a real indexed folder.

After the #31 review:

- Change-related wording now depends on when the error arrived: refused before any write, partway through a batch, or partway through an Undo.
- `historyRequired` now says the file was changed but Undo isn't available, with no Try again, because the native writer reports it only after a write.
- `npm test`: 125 passed, 9 todo. New cases: no message claims nothing changed after a write or a partial Undo, partway messages keep earlier changes, and a spent plan is never retried.
- In the browser, `historyRequired`, `undoConflict` and `targetChanged` show the new wording, and a fast file open no longer announces "Reading file…" (it waits 500 ms).

Not verified: real native failures (only simulated ones), the folder-opened success message (it needs the native folder picker), screen readers, and the Tauri webview.

### Relationships list and Graph (2026-10-09, issue #21)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 119 passed, 9 todo. New cases cover:
  - no connected file dropped (12 of 12 listed);
  - links in both directions folded into one entry with all evidence;
  - index and opened-file links merged without repeats;
  - exact duplicates from the index and from hashes;
  - links and identical bytes never labelled as AI output;
  - model provenance labelled and flagged for checking;
  - passage highlighting after multi-byte characters;
  - stale passages refused.
  - picking a file elsewhere clearing both the trail and an old highlight (fixed after review; the navigation state is a tested reducer).
- Browser preview (sample files) in headless Chromium at 1280×850 and 700×800:
  - Graph lists 10 links;
  - `project-plan.md` shows 5 related files, including the exact duplicate `project-plan-copy.md` and the Filipino note `tala-sa-proyekto.md`;
  - opening an excerpt shows the source with the passage highlighted and focused;
  - "Back to" returns to the origin's Related tab;
  - picking a file from a list clears the trail;
  - Tab reaches each related file and each excerpt in order;
  - no horizontal scroll at 700px.

After review: the folder index is read only when Related or Graph is shown, `refresh()` re-reads it, and an indexed folder uses only the native byte-verified duplicate groups.

Not verified: a real folder in the Tauri app (the index path was only type-checked), screen readers, and a PDF passage with a page number.

### Retry backoff for unreadable documents (2026-10-09, issue #28)

Checked on Linux (x86-64 VM, 8 vCPUs, 7 GiB RAM) with Rust 1.99.0 and Node.js 24.15.0. This host has no WebKit/GTK development libraries, so the Tauri crate can't be built here. The native suites ran in a scratch crate that compiles every module in `src-tauri/src` except `lib.rs` (the Tauri command glue), with the same dependency versions from `Cargo.lock`:

- Native tests: 129 passed, 5 ignored on this branch, and 155 passed, 2 ignored after merging `main` with #12 (its writer, Ripple and Organize suites included; the migration is now `004_retry_backoff.sql`). This includes the two #11 retry tests unchanged, plus new tests for:
  - corrupt PDFs that are no longer re-extracted from the fourth scan, while still counted as `failed`;
  - a file fixed without a visible change, which recovers once the 10-minute wait ends;
  - a read-only toggle that retries at once;
  - an extractor-version change that retries at once;
  - the scan flag and `recheck_documents`, including refused unknown and other-folder ids;
  - the backoff schedule and its 6-hour cap;
  - the count restarting when the file changes;
  - a clock that went back;
  - a successful read clearing the retry state;
  - a deferred stale document that stays searchable.
- `lib.rs`, including the new `recheck_documents` command and the `recheckUnreadable` argument, was not compiled on this host; CI's Windows and macOS jobs build it. The Windows attribute signature is likewise built and tested only there.
- `npm run format:check`, `npm run check`, `npm test` (104 passed, 9 todo, after merging `main` with #12 and #17) and `npm run build`: passed.

Rescan time (`measure_rescan_with_corrupt_pdfs`, ignored in CI). Each folder is the 16 fixture documents plus 30 PDFs that always fail, and each figure is the mean of 5 rescans after the first three failures. "Every scan" is the old behaviour, forced with `recheckUnreadable`.

| 30 failing PDFs                              | Build   | Every scan | With backoff |
| -------------------------------------------- | ------- | ---------- | ------------ |
| `%PDF-1.4` + 1 MiB of garbage                | release | 356 ms     | 0.9 ms       |
| `%PDF-1.4` + 1 MiB of garbage                | debug   | 3.3 s      | 2.9 ms       |
| One page that inflates past the 16 MiB limit | release | 260 ms     | 0.9 ms       |
| One page that inflates past the 16 MiB limit | debug   | 5.5 s      | 2.9 ms       |

These are single runs on one VM with a warm file cache, not Windows or macOS figures. Full-folder indexing time and database size remain unmeasured.

After TJ's review of PR #30: "Check again" (and a refresh after a Folio write) now removes a record only when its path is really absent. A file inside a folder that can't be read right now (permissions, an offline cloud or network folder), or whose metadata can't be read, keeps its record and index data, is recorded as a read failure (`stale` if it was indexed) and backs off like any unreadable file. One such file no longer aborts the rest of the re-check. TJ's repro passes on Linux (157 passed, 2 ignored with `main` and #12 merged), along with a test that a deleted file is still removed. [CI run 37949475975](https://github.com/duckycodess/folio/actions/runs/37949475975), before this fix, passed on Windows (146 passed, 2 ignored) and macOS (154 passed, 2 ignored).

Developer note: if you ran this branch before the migration was renumbered to `004_retry_backoff.sql`, your index records the retry columns as version 3 and will fail to open with "duplicate column name: retry_failures". Delete `folio.sqlite` (and its `-wal`/`-shm` files) in Folio's app-data folder; the next scan rebuilds it. Nothing has shipped, so no user database is affected.

### File table and reader (2026-10-09, issue #17)

Checked on macOS with Node.js 26.10.0:

- `npm run format:check`, `npm run check` and `npm run build`: passed.
- `npm test`: 104 passed, 16 todo. The new cases cover which document the reader and Organize may use: nothing until a file is chosen, never a file the current search excludes, files opened from Graph, and none in Organize or Ask & Act.
- Browser preview in headless Google Chrome via Playwright, at 1280×850, 1024×768, 700×800, and 640×425 at device scale 2:
  - nothing is selected or opened at startup;
  - at 1280 and 1024 the table and the reader are both on the first screen, and with the reader open the table keeps Name, Location and Modified;
  - at 700px and 200% zoom the reader replaces the table, Back returns to it with focus on the row, and there's no horizontal scroll or clipped heading;
  - the open row has `aria-selected="true"` and a spoken name with its location, type, date and size;
  - a long Filipino/Taglish file name and folder path injected into a row truncate with an ellipsis, without page overflow, and the row's tooltip carries the full path;
  - a search that excludes the open file closes the reader, and clearing the search brings it back;
  - after review: with that search active, Organize offers only the matching files instead of a rename form for the hidden one (open `project-plan.md`, search "budget", go to Organize → only `budget-notes.md` is offered);
  - the reader shows the contents with a Read-only badge, the full path, and the source ("Sample file bundled with Folio").
- Desktop no-folder state, simulated in the browser by setting the flag the Tauri API reads: Home and Files show Olio with "Add folder" and "Look at sample files", and the latter lists the 15 samples under "Showing sample files".

Not verified: the real Tauri webview, native folder picking, a real folder with no readable files (the empty-folder state was not rendered), text-PDF pages (PDF text extraction doesn't exist yet, so the reader says so), screen readers, and column sorting (not built). Sample files have no modification time, so their Modified column shows "—".

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

Follow-up after the #25 design review, checked the same way at 1280×850 and 700×600:

- the dark tokens match `docs/design.md`;
- Escape closes the document panel and returns focus to the row that opened it;
- only one file row is in the Tab order, and arrow keys move between rows;
- at 700×600 the Home title is visually hidden but still a heading for screen readers, and six file rows are on the first screen;
- picking a folder before the sample files finish loading can no longer show the samples as that folder (code fix; there's no DOM test environment to cover it automatically).

Second review round on PR #26, checked the same way (headless Chrome, sample fixtures) at 1280×850, 700×800 and 640×425 at device scale 2:

- with a document open at 700px and 200% zoom, the search field and notices stay visible above it, ⌘K focuses search, opening a document moves focus to its heading, and there's no horizontal scroll;
- Organize no longer opens the reader; the rename form stays visible with a "Choose a different file" control, and the typed name is cleared when the file changes;
- Escape in the search field clears it without closing the document; Escape elsewhere still closes it and returns focus to its row;
- following a Related link moves focus to the new document's heading;
- the list's single Tab stop follows the arrow-key focus;
- startup shows "Loading files…" instead of the empty-folder message;
- non-error notices are announced through one always-present live region (DOM check only; not tested with a screen reader);
- with reduced motion, the indeterminate progress bar is a still, half-opaque fill;
- `npm test`: 92 passed, 16 todo, adding cases for the search shortcut on Cyrillic, Greek and Dvorak layouts.

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

These changes were written on a Linux host without a Rust toolchain or the Tauri system libraries, so `cargo test` was not run locally. [Run 37947687958](https://github.com/duckycodess/folio/actions/runs/37947687958), for commit `8cd03f0`, passed all three jobs: frontend, `desktop-check (macos-latest)` with 144 native tests passed and 1 ignored (including both Unix permission tests), and `desktop-check (windows-latest)` with 136 passed and 1 ignored. Locally, `npm run check`, `npm test` (177 passed, 25 `todo`) and `npm run build` passed. The native suite was not run on Linux.

Local inference, native packaging, and actual performance/size measurements remain unverified. Indexing time and database size have not been measured.

## Product-context refresh

On 2026-10-09, the user confirmed the settled scope and authorized Impeccable init plus a direct main-branch push. The existing `docs/product.md` was updated in place with product-schema version 1, the webview platform classification, users, purpose, positioning, operating context, evidence, product principles and accessibility requirements. No competing root `PRODUCT.md` or replacement visual world was created.

The named team plan now gives TJ most checking and integration/release coordination, Dann local AI, Gab native workspace/actions and Louise the replacement UI. The full MVP and current UI replacement remain implementation work; this documentation refresh does not complete any of those features.

Documentation-refresh validation passed: Impeccable product schema 1 and the webview platform value, all 15 local Markdown references, preservation of the four original scope sections, named ownership and all ten issue links, and Prettier formatting for the three changed documentation files. Application and native tests were not rerun for this documentation-only refresh.
