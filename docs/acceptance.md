# Acceptance and test matrix

## Full demo

Disconnect internet after setup. On the prepared project corpus, request in Taglish: **“Hanapin yung project plan at palitan ang deadline na October 20 to October 23.”** Verify the retrieved target, deadline evidence, connected documents, exact diff, related review candidates, approval, real save, local refresh, and conflict-aware undo. Then show Search, Organize, and Summarize directly through journeys A and B.

## Required evidence

| Case                                 | Expected result                                                             |
| ------------------------------------ | --------------------------------------------------------------------------- |
| English search against Filipino note | Relevant source retrieved; path and excerpt shown                           |
| Filipino search against English plan | Relevant source retrieved without renaming the query language               |
| Taglish action request               | Correct operation, target, and arguments; no write during interpretation    |
| Individual summary                   | Important facts preserved; passages cited; no unsupported claims            |
| Unknown answer                       | Honest insufficient-evidence result                                         |
| Same subject, unrelated fact         | Similarity edge may appear; no claim that the unrelated edit must propagate |
| Changed deadline in linked note      | Relevant old-deadline passage flagged for review                            |
| Ambiguous target                     | User selects the file before a mutation plan is created                     |
| Unapproved plan                      | Mutation refused                                                            |
| Expired plan or external target edit | Old approval refused; new preview required                                  |
| Outside root or escaping symlink     | Access refused                                                              |
| Rename collision                     | Refused or resolved through a new explicit plan; existing file preserved    |
| Failed save                          | No false success or false index update; recoverable preview/history state   |
| Undo after external edit             | Conflict shown; unrelated edits preserved                                   |
| Model unavailable                    | Normal browsing works; AI action shows setup guidance                       |
| Changed embedding model              | Separate/rebuilt index; no mixed vector comparison                          |
| Exact duplicate                      | Byte/content-hash identity, not similarity alone                            |

## Where each case stands

Contract and safety checks run today; they prove the agreed boundary, not a
completed feature. Cases that need the native writer or a local model are
listed as pending rather than mocked.

| Case                                 | Checked today                                                                             | Still pending                          |
| ------------------------------------ | ----------------------------------------------------------------------------------------- | -------------------------------------- |
| Outside root or escaping symlink     | `src-tauri/src/workspace.rs`, `src-tauri/src/plan.rs` against temporary synthetic folders | —                                      |
| Unapproved plan                      | `src-tauri/src/plan.rs`, `src/domain/approval.test.ts`                                    | Real write refused on disk (#5)        |
| Expired plan or external target edit | `src-tauri/src/plan.rs`, `src/domain/approval.test.ts`                                    | —                                      |
| Rename collision                     | Preflight refuses and the existing file is read back unchanged                            | Real rename on disk (#5)               |
| Ambiguous target                     | Plans carry explicit operations only; evidence cannot become one                          | Assistant target selection (#6, #7)    |
| Failed save                          | Batch outcomes: `failed` stops the batch, earlier successes stay durable                  | A real failed write (#5)               |
| Undo after external edit             | Whole-batch preflight refuses and names the blocking file                                 | A real reversal (#5)                   |
| Changed embedding model              | Embedding-space fingerprints are compared, never mixed                                    | A real second index (#4)               |
| Exact duplicate                      | Content hashes are produced natively on read                                              | Duplicate grouping (#3)                |
| English/Filipino/Taglish retrieval   | Keyword retrieval over the synthetic corpus, labelled as keyword                          | Cross-language semantic retrieval (#4) |
| Individual summary, unknown answer   | Contract shapes only (`GenerationOutcome`)                                                | Local generation (#4)                  |
| Changed deadline in linked note      | Explicit-reference evidence with both revisions                                           | Ripple over semantic neighbours (#5)   |
| Model unavailable                    | Provider error codes are frozen                                                           | Real adapter behaviour (#4, #8)        |

## Model Lab

Use `fixtures/benchmark-cases.json` as an initial labelled suite. Interpretation, retrieval, generation, and editing have separate scores. Compare only supported tasks under the same document/context conditions. No self-graded aggregate quality score.

Record exact model revision, file quantization, runtime version, OS/CPU/RAM, context/output budget, prompt/test revision, cold/warm state, task duration, success criteria, and model disk bytes. RAM must identify the measured process(es); unavailable metrics remain unavailable. Test models sequentially; retain results after model removal.

## Budget verification

Measure actual installed app/runtime/tokenizer/model files, index bytes, cache/history bytes, cold load time, peak process memory, and normal search responsiveness on the 8-GB target. The original under-1-GB target is not demonstrated by adding weight-file sizes alone.
