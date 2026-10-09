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

Multilingual embedding integration, semantic search, SQLite connection, text-PDF extraction, model lifecycle/downloads, local generation, AI summaries, command interpretation, shared-fact discovery, durable apply/undo/history, incremental indexing, and real Model Lab results.

No AI or save completion should be presented until the corresponding native/provider implementation succeeds. Model sizes, installed size, memory targets, and platform support remain subject to measurements.

## Issue #4 planning follow-up

Recorded accepted model-selection, independent acceptance-suite, clarification, and full/partial-summary policies in `docs/grill-with-docs.md`; added Partial Summary vocabulary to `GLOSSARY.md`. The combined issues #4/#8 follow-up also settled objective checks plus TJ factual review (unreviewed summaries are not passes), and disposable benchmark workspaces with native approval for actual apply tests. Earlier Q5–Q7 proposals remain unaccepted. No inference or runtime functionality was implemented. This follow-up changed documentation only; no code tests or model benchmarks were run.

## Verification

Checked on 2026-10-09 with Node.js 24.19.0:

- `npm run check`: passed.
- `npm test`: 9 tests passed, covering approval/expiry/current-content checks, required history, explicit-reference containment, Filipino keyword retrieval, and embedding-space fingerprints.
- `npm run build`: passed; the production frontend compiles.
- SQLite migration: executed in memory with Python's SQLite; Filipino FTS matching, insert/update/delete triggers, embedding revision separation, foreign-key guards, and cascading cleanup passed.
- `npm run tauri info`: configuration recognized. Native builds/tests were not run because Rust/Cargo and Linux desktop libraries are unavailable here. Windows/macOS CI jobs are configured but have not run yet.
- Browser smoke checks were attempted but could not start: the browser binary is absent and its download returned an invalid archive. Visual behavior and native folder-picker behavior still require verification.

Local inference, PDF extraction, filesystem apply/undo, native packaging, and actual performance/size measurements remain unverified and unimplemented as described above.
