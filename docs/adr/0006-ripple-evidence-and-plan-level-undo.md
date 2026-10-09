# Flag only evidenced Ripple candidates and undo whole plans

A deadline edit must surface the passages that may need review without implying that every document mentioning the same date is affected. A document is Ripple evidence only when it links to or from the edited document and mentions the replaced value. Byte-identical copies and non-link relationships are listed as similarity-only. Documents that merely share the value are not listed. Ripple never adds operations.

Approved plans expire after ten minutes. An edit replaces a passage that must occur exactly once. Destination folders must already exist.

If a write fails, the earlier writes in the plan are reverted where the files still match what Folio wrote, and the result reports exactly what was and wasn't restored rather than claiming atomicity. Undo reverses a whole plan as a direct user action without a separate approval. It first checks every file against what Folio wrote and refuses with no changes if any differ. Recoverable content is kept for the last 100 applied plans.
