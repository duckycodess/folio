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

## Model Lab

Use `fixtures/benchmark-cases.json` as an initial labelled suite. Interpretation, retrieval, generation, and editing have separate scores. Compare only supported tasks under the same document/context conditions. No self-graded aggregate quality score.

Record exact model revision, file quantization, runtime version, OS/CPU/RAM, context/output budget, prompt/test revision, cold/warm state, task duration, success criteria, and model disk bytes. RAM must identify the measured process(es); unavailable metrics remain unavailable. Test models sequentially; retain results after model removal.

## Budget verification

Measure actual installed app/runtime/tokenizer/model files, index bytes, cache/history bytes, cold load time, peak process memory, and normal search responsiveness on the 8-GB target. The original under-1-GB target is not demonstrated by adding weight-file sizes alone.
