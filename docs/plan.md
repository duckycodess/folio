# Four-person build plan

The original time budget was approximately 12 hours for four people. Rebase this schedule against actual remaining time when work begins. This is an implementation plan, not a claim that workers have been dispatched.

## Shared first hour

All four people read the scope and contracts. Build the native shell on at least one Windows and one macOS machine immediately. Freeze contract names and error shapes before parallel implementation. Run the current frontend preview to confirm SOS and journeys A/B/C remain visible.

Track identifiers describe work boundaries, not person numbers. The user assigned Person 1 to TJ, Person 2 to Dann, Person 3 to Gab and Person 4 to Louise. TJ gets most checking; Louise owns the replacement UI. Gab owns both native implementation tracks.

| Track | Owner             | Work                                                                                                                                            | Dependencies                                                          | Acceptance                                                                                                                            |
| ----- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| T0    | TJ — Person 1     | Shared contracts, safety-test baseline, integration coordination, offline/accessibility checks, Windows/macOS package and resource verification | Contracts first; final checks consume completed implementation tracks | Stable identities/error shapes; acceptance matrix evidence; real native/offline/package results; honest measured budgets              |
| T1    | Gab — Person 3    | Workspace permissions, parsing, SQLite/FTS5, hashes, incremental indexing, duplicate groups                                                     | Shared contracts                                                      | Authorized-folder scan; TXT/MD and text-PDF extraction; unchanged files not reprocessed; exact duplicates verified                    |
| T2    | Dann — Person 2   | Multilingual embeddings, local generation runtime, grounded answers/summaries, small Model Lab                                                  | Contracts; T1 chunks                                                  | English/Filipino/Taglish and cross-language cases pass; summaries cite passages; no hosted inference; real metrics only               |
| T3    | Louise — Person 4 | Replace current starter UI; SOS/file/source views, evidence, graph, organize preview, assistant and Model Lab presentation                      | Contracts; adapters from T1/T2/T4; fixture adapters allow early work  | A/B/C use the same identities; readable keyboard-accessible UI; actionable provider/error states; exact preview and honest save state |
| T4    | Gab — Person 3    | Native plan/approval/apply/history/undo, relationships and Ripple evidence                                                                      | Contracts; T1 identity/hash; T2 interpretation                        | No write before approval; stale plan rejected; save + local refresh; related files only flagged; undo conflict checked                |

TJ owns integration coordination and final acceptance review across all tracks. Implementation owners run checks relevant to their own changes. Louise consumes adapters instead of editing native filesystem logic; Dann consumes chunks instead of changing parsers. Coordinate contract changes before merging.

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

These IDs remain local plan references. The [MVP index](https://github.com/duckycodess/folio/issues/1) links their real GitHub tickets:

| Owner             | Plan work                                                                   | GitHub tickets                                                                                                                                                  |
| ----------------- | --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| TJ — Person 1     | F01 shared contracts/safety baseline; F08 integration; F10 packages/budgets | [#2](https://github.com/duckycodess/folio/issues/2), [#9](https://github.com/duckycodess/folio/issues/9), [#10](https://github.com/duckycodess/folio/issues/10) |
| Dann — Person 2   | F03 local retrieval/generation/interpretation; F09 Model Lab                | [#4](https://github.com/duckycodess/folio/issues/4), [#8](https://github.com/duckycodess/folio/issues/8)                                                        |
| Gab — Person 3    | F02 workspace/indexing; native F05/F06/F07 relationships/actions/history    | [#3](https://github.com/duckycodess/folio/issues/3), [#5](https://github.com/duckycodess/folio/issues/5)                                                        |
| Louise — Person 4 | F04 replacement UI and workflow presentation                                | [#6](https://github.com/duckycodess/folio/issues/6), [#7](https://github.com/duckycodess/folio/issues/7)                                                        |

The UI must be replaced while preserving SOS and the existing shared/native boundaries. All tickets were created unqueued; this plan does not authorize auto-dispatch via `agent:ready`. Prerequisite issue numbers are recorded in the tickets, but automatic dependency-graph wiring was unavailable during publication because Folio had no configured Beads graph.
