# Graph test workspace: expected connections

Point Folio at `fixtures/graph-test/workspace/` (not this folder, so this file stays
out of the index). The scenario is one thesis group studying Laguna de Bay water quality,
with English, Filipino, and Taglish documents.

## Explicit references (Markdown links, deterministic)

| From | To | Notes |
| --- | --- | --- |
| thesis/proposal.md | thesis/methodology.md | |
| thesis/proposal.md | thesis/literature-review.md | `./` prefix |
| thesis/proposal.md | thesis/sampling-sites.md | |
| thesis/proposal.md | schedule/timeline.md | `../` cross-folder |
| thesis/proposal.md | budget/budget-plan.md | |
| thesis/methodology.md | thesis/sampling-sites.md | |
| thesis/methodology.md | thesis/literature-review.md | |
| thesis/literature-review.md | thesis/proposal.md | `#objectives` anchor; cycle with proposal |
| schedule/timeline.md | thesis/proposal.md | `#objectives` anchor |
| schedule/timeline.md | budget/budget-plan.md | |
| budget/budget-plan.md | schedule/timeline.md | cycle with timeline |
| schedule/group-chat-notes.md | budget/budget plan draft.md | percent-encoded `%20` |
| community/coastal-cleanup.md | thesis/sampling-sites.md | |
| misc/reading-list.txt | thesis/literature-review.md | link inside a .txt file |
| archive/proposal-copy.md | schedule/timeline.md, budget/budget-plan.md | only its `../` links resolve from `archive/` |

## Links that should not resolve

- archive/old-outline.md links to `../thesis/outline.md`, which does not exist.
- archive/proposal-copy.md's links to `methodology.md`, `./literature-review.md` and
  `sampling-sites.md` resolve to `archive/...`, which do not exist. Its `../schedule/` and
  `../budget/` links do resolve.

## Exact duplicate

- archive/proposal-copy.md is a byte-identical copy of thesis/proposal.md.

## Shared fact candidates (dates and amounts)

- October 24, 2026 (defense): proposal, methodology, timeline, group-chat-notes
  ("Oktubre 24"), metodolohiya-buod ("Oktubre 24").
- October 18, 2026 (cleanup): timeline, group-chat-notes, coastal-cleanup,
  paanyaya-sa-paglilinis ("Oktubre 18").
- October 10, 2026 (data collection): methodology, timeline, metodolohiya-buod.
- October 21, 2026 (manuscript): timeline, group-chat-notes, budget-plan.
- PHP 12,500 (budget): proposal, budget-plan, group-chat-notes.
- PHP 11,800 in "budget plan draft.md" differs from PHP 12,500. It should surface for
  review, not as a confirmed contradiction.

## Similarity (needs a local embedding model)

- thesis/methodology.md and thesis/metodolohiya-buod.md (English and Filipino versions).
- community/coastal-cleanup.md and community/paanyaya-sa-paglilinis.md (English and Filipino).
- budget/budget-plan.md and budget/budget plan draft.md (near-duplicates).
- literature-review, proposal, coastal-cleanup (shared water-quality topic).

## Isolated

- misc/adobo-recipe.md has no links, no shared facts, and an unrelated topic. It should
  have no connections.
