# Four-person build plan

The original time budget was approximately 12 hours for four people. Rebase this schedule against actual remaining time when work begins. This is an implementation plan, not a claim that workers have been dispatched.

## Shared first hour

All four people read the scope and contracts. Build the native shell on at least one Windows and one macOS machine immediately. Freeze contract names and error shapes before parallel implementation. Run the current frontend preview to confirm SOS and journeys A/B/C remain visible.

| Track | Owner    | Work                                                                                           | Dependencies                                   | Acceptance                                                                                                              |
| ----- | -------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| T1    | Person 1 | Workspace permissions, parsing, SQLite/FTS5, hashes, incremental indexing, duplicate groups    | Shared contracts                               | Authorized-folder scan; TXT/MD and text-PDF extraction; unchanged files not reprocessed; exact duplicates verified      |
| T2    | Person 2 | Multilingual embeddings, local generation runtime, grounded answers/summaries, small Model Lab | Contracts; T1 chunks                           | English/Filipino/Taglish and cross-language cases pass; summaries cite passages; no hosted inference; real metrics only |
| T3    | Person 3 | SOS UI, file views, evidence, graph, organize preview, assistant entry                         | Contracts; adapters from T1/T2/T4              | All three journeys run through the same document identities; missing providers/error states are clear                   |
| T4    | Person 4 | Native plan/approval/apply/history/undo, Ripple evidence, platform smoke checks                | Contracts; T1 identity/hash; T2 interpretation | No write before approval; stale plan rejected; save + local refresh; related files only flagged; undo conflict checked  |

T4 owns integration coordination. T3 should consume adapters instead of editing native code; T2 should consume chunks instead of changing parsers. Coordinate contract changes before merging.

## Delivery slices

1. **Hours 0–2:** native boot + authorized scan + fixture indexing. One complete file can be read.
2. **Hours 2–5:** multilingual retrieval and single-file summary, paths/passages and explicit relationships visible.
3. **Hours 5–8:** approved TXT/Markdown change with Ripple preview, save, local refresh, and undo; organize rename/move and hashes.
4. **Hours 8–10:** full workflow A/B/C integration, fixed bilingual tests, basic Model Lab, Windows/macOS smoke checks.
5. **Final 2 hours:** freeze features, rehearse offline demo, repair failures, package and record actual limitations.

If ahead, add automatic virtual collections and project-wide summaries. Do not spend integration time on OCR, Office editing, cloud sync, full contradiction detection, phone packaging, new agent frameworks, or multiple specialized LLMs.

## Task dependencies

- `F01` Shared contracts and desktop boot — unblocks all tracks.
- `F02` Workspace/index/extraction — depends on F01.
- `F03` Multilingual retrieval and generation — depends on F01 and F02 chunks.
- `F04` File/source/SOS views — depends on F01; fixture adapter allows early work.
- `F05` Relationships and Ripple — depends on F02; generated explanations depend on F03.
- `F06` Plan/approval/apply/history/undo — depends on F01 and F02 identity/hash.
- `F07` Organize actions/duplicates — depends on F02 and F06.
- `F08` Offline bilingual integration/demo — depends on F03–F07.
- `F09` Small Model Lab — depends on F03 and actual task harness.
- `F10` Windows/macOS package verification — starts at F01 and completes after F08.

These IDs are local plan references, not existing GitHub issues. The plan does not authorize auto-dispatch via `agent:ready`.
