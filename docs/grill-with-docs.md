# Grill-with-docs decision record

This records the already completed Folio interview on 2026-10-09. The user authorized turning the settled answers into docs and a public starter repository. Do not restart the same interview.

## Settled decisions

| Question                | Decision                                                                                                            |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------- |
| 1 — Audience            | Students first; general population remains supported through general file workflows.                                |
| 2, 6 — Platforms        | Windows and macOS desktops first; mobile later.                                                                     |
| 3 — Storage             | Target under 1 GB for the default installed app and models; variable documents/index/history are separate.          |
| 4 — Offline             | All core features work offline after initial setup/model download.                                                  |
| 5 — Capacity            | Four people, approximately 12 hours remaining at the time of scoping.                                               |
| 7 — Pipeline            | Request → find files → understand content → discover related files → edit/analyze impact → approve/save/local sync. |
| 8 — Formats             | Text PDFs, TXT, Markdown; content editing limited to TXT/Markdown.                                                  |
| 9 — Stack               | Assistant may choose; AI-assisted development is expected.                                                          |
| 10 — Model Lab          | Small settings screen and fixed-task comparison; richer analytics later.                                            |
| 11 — Sync               | Local index/graph refresh, not cross-device synchronization.                                                        |
| 12 — Ripple             | Preview target edit and related-file evidence; flag related files rather than changing them.                        |
| 13 — Relationships      | Distinguish similarity, explicit references, and shared fact candidates.                                            |
| 14 — Demo data          | 10–20 prepared actual local documents with repeated project-deadline evidence.                                      |
| 15 — Languages          | English-only was rejected. Filipino is core; English and Taglish must also be supported.                            |
| 16 — Feature priorities | Exact duplicates, suggested filenames, history/undo required; smart collections and project-wide summaries stretch. |
| 17 — Multilingual UX    | English UI labels; multilingual requests/content/responses; three-language initial test coverage.                   |
| 18 — Organization       | Automatic analysis and virtual grouping; approved physical file changes.                                            |
| Workflow correction     | Keep SOS and all three image workflows. Individual summaries and Organize are core features.                        |

## Organize virtual collections (2026-10-10, issue #78)

Gab takes #78 from Dann, and Dann reviews the model parts. Rationale: [ADR 0017](adr/0017-virtual-collections-kept-natively-without-a-plan.md).

| Question        | Decision                                                                                                          |
| --------------- | ----------------------------------------------------------------------------------------------------------------- |
| First slice     | Suggested collections only. Model filenames and destinations are follow-up PRs.                                   |
| Embeddings      | #4's in-memory snapshot vectors, within one space. Switch to persistent vectors when #27/#46 write them.          |
| Keeping         | Stored natively without a plan, approval or Activity entry; no file changes.                                      |
| Members         | Follow Folio's own rename/move/delete/undo; outside changes leave a missing member; never re-matched by hash.     |
| Naming          | Local model, members' main language, cited, editable; no generation model means no name until the user types one. |
| Overlap         | Kept collections may share documents; one analysis's suggestions don't.                                           |
| Organize target | A kept collection can be analyzed; duplicates and filename suggestions are limited to its members.                |
| UI              | Built in the same PR; Louise reviews.                                                                             |

## Technical decisions delegated to implementation

Choose Tauri 2 + React/TypeScript with a small native Rust core, SQLite/FTS5, and separate local embedding/generation adapters. This choice favors the installation target and shared desktop shell. Validate native builds immediately; retain a frontend development preview to keep UI work unblocked.

Candidate models are quantized multilingual-E5-small for retrieval and a small quantized Qwen model for generation/commands. Model choice is gated on English, Filipino, and Taglish task correctness plus actual size and latency. These are candidates, not measured recommendations. MiniLM's English-oriented default was replaced when Filipino became mandatory.

## Continue the interview when needed

Use [upstream grill-with-docs](https://github.com/mattpocock/skills/blob/main/skills/engineering/grill-with-docs/SKILL.md), which combines [grilling](https://github.com/mattpocock/skills/blob/main/skills/productivity/grilling/SKILL.md) and [domain modeling](https://github.com/mattpocock/skills/blob/main/skills/engineering/domain-modeling/SKILL.md).

Ask only unresolved product decisions whose prerequisites are settled. Give a recommended answer and await the user's choices. Look up environmental facts rather than asking the user to guess. Record resolved domain terms immediately in the root glossary. Write an ADR only for a consequential, non-obvious trade-off. Implementation details belong in architecture/setup, not the glossary.

## Issue #4 follow-up decisions

The user accepted the recommendations for follow-up questions 1–4:

- Select the smallest local embedding/generation pair that separately passes required multilingual retrieval, grounded summaries, and typed interpretation. English performance cannot compensate for failed Filipino/Taglish tasks. Report an unmet under-1-GB target honestly rather than silently weakening required functionality; the target is not an absolute release gate.
- Require the prepared demonstration plus a small frozen, labelled acceptance suite with held-out paraphrases, distractors, ambiguity, insufficient evidence, malicious passages, missing/corrupt models, and cancellation. TJ independently checks outcomes; do not use model self-grading.
- Ask for file selection when identity is ambiguous and clarification when the intended operation/edit is ambiguous. Invalid generated output creates no plan; never silently select the highest-ranked file for mutation.
- Full-file summaries process the whole document in bounded stages with citations traceable to original passages. Incomplete processing produces an explicitly labelled Partial Summary with stated coverage. Whole-file coverage does not promise that every fact appears in the concise result. Specific file questions may use retrieved passages.

The user also accepted shared recommendations Q8–Q9 for issues #4 and #8:

- Automatically check objective expectations such as targets, exact edits, and citation locations. Preserve generated outputs and supporting evidence for TJ's factual review. Summary correctness remains Not reviewed until reviewed; valid citations or required-fact matches alone do not establish factual correctness. No model self-grading.
- Run benchmarks in an isolated disposable workspace containing copies of the test corpus, reset between models. Proposal tests do not write by default. Any actual apply test uses Gab's native approval engine and explicit approval; benchmarks do not bypass approval or modify the user's original workspace.

Earlier Q5–Q7 recommendations (mandatory-test completion gate, processing-limit behavior, and competing-generation UX) remain proposed defaults, not accepted decisions. Implementation may proceed within existing documented boundaries without treating these proposals as settled.

These decisions do not authorize implementation, issue queueing, or worker dispatch.

## Optional-model expansion interview

After removing the download allowance cap, the user identified correctness and efficiency as the reasons to consider more models. Larger models remain optional, explicitly downloaded packs; the under-1-GB default-install target is unchanged. The removed allowance applies to model download/on-disk size, not runtime RAM. The 8 GB target means total device RAM shared with the OS and other applications; CPU-only operation must not require a dedicated GPU. Removal of the download cap does not waive that evaluation target or establish model quality. The user also accepted measuring task correctness, response time, process memory, and retry needs separately: prioritize acceptable correctness, then efficiency among models that meet it. Generation and embedding adapters are evaluated independently; expand only where observed weaknesses justify it, using current candidates as the baseline. New candidates enter an evaluation-only shortlist rather than immediately becoming selectable supported packs. Promotion requires review of required task outcomes and resource measurements; if none qualifies, report that no suitable candidate has been established. Evaluation runs on remote Windows/macOS CI; those measurements describe the runner, not proof of the 8-GB device target. The interview supports evidence-driven optional-model expansion, but the final summary and any concrete new candidate shortlist still require user confirmation before additions.

## Open verification items, not interview questions

- Actual generation/command quality of the selected small model in Filipino and Taglish.
- Cross-language retrieval quality after embedding quantization.
- Complete app/runtime/tokenizer/model installed size and peak process memory.
- Windows and macOS native packaging and folder-picker behavior.
- Reliable PDF extraction and model runtime integration.
- Whether the remaining build time permits stretch features after the full SOS workflow passes.
