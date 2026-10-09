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

## Technical decisions delegated to implementation

Choose Tauri 2 + React/TypeScript with a small native Rust core, SQLite/FTS5, and separate local embedding/generation adapters. This choice favors the installation target and shared desktop shell. Validate native builds immediately; retain a frontend development preview to keep UI work unblocked.

Candidate models are quantized multilingual-E5-small for retrieval and a small quantized Qwen model for generation/commands. Model choice is gated on English, Filipino, and Taglish task correctness plus actual size and latency. These are candidates, not measured recommendations. MiniLM's English-oriented default was replaced when Filipino became mandatory.

## Continue the interview when needed

Use [upstream grill-with-docs](https://github.com/mattpocock/skills/blob/main/skills/engineering/grill-with-docs/SKILL.md), which combines [grilling](https://github.com/mattpocock/skills/blob/main/skills/productivity/grilling/SKILL.md) and [domain modeling](https://github.com/mattpocock/skills/blob/main/skills/engineering/domain-modeling/SKILL.md).

Ask only unresolved product decisions whose prerequisites are settled. Give a recommended answer and await the user's choices. Look up environmental facts rather than asking the user to guess. Record resolved domain terms immediately in the root glossary. Write an ADR only for a consequential, non-obvious trade-off. Implementation details belong in architecture/setup, not the glossary.

## Open verification items, not interview questions

- Actual generation/command quality of the selected small model in Filipino and Taglish.
- Cross-language retrieval quality after embedding quantization.
- Complete app/runtime/tokenizer/model installed size and peak process memory.
- Windows and macOS native packaging and folder-picker behavior.
- Reliable PDF extraction and model runtime integration.
- Whether the remaining build time permits stretch features after the full SOS workflow passes.
