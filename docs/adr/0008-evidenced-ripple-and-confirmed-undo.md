# Flag only evidenced Ripple candidates and undo only a confirmed preview

A deadline edit must surface the passages that may need review without implying that every document mentioning the same value is affected. A document is Ripple evidence when it links to or from the edited document, or is a shared-fact candidate for it, and mentions the replaced phrase as a whole phrase. Dates also match when written with English, abbreviated or Filipino month names, and "October 20" does not match "October 2026". Similarity relationships and byte-identical copies are similarity-only. Documents that merely share the value are not listed. Candidates carry the relationship type and provenance, are capped at 25, and never become operations. The replaced phrase comes from the edit's diff, widened to whole words and date phrases, unless the caller supplies it.

Undo is a physical change, so ADR 0001's preview-and-approval rule applies to it too. Folio first shows the Undo preflight, then reverses only the exact pending entries the user confirmed. Any conflict, or any difference from the confirmed entries, changes nothing. An Undo that stops partway keeps what it reversed and leaves the rest pending for a fresh preview. Consistent with ADR 0006, a failed apply keeps its earlier successes and offers Undo instead of rolling back on its own.

Edits keep their previous content for the 100 most recent applied plans. Renames, moves and creates need no stored content and remain undoable.

Decided with Gab on 2026-10-09 in response to the PR #12 review.
