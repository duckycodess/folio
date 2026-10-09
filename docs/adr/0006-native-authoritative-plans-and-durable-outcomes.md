# Keep plans, approval and per-operation outcomes in the native core

A preview the user approves must be the thing that gets applied, and a batch
that stops halfway must say exactly what changed. The starter tracked a single
approval and one history identifier for a whole plan, which cannot express a
batch that stopped at its second operation.

The native core now issues plan identities and a `sha256` digest over canonical,
length-prefixed plan bytes, and an approval binds to that identity _and_ that
digest. A plan identity the UI invented is unknown; a digest it invented does
not match; operations changed after approval are stale. Preflight checks every
target before anything is written, including the explicit absence of each
destination, so a rename cannot overwrite a file.

A batch records one durable outcome per operation — `succeeded`, `failed`,
`cancelled` or `notStarted` — with the history entry for each success. A failure
stops the batch and leaves the rest `notStarted`; a cancellation lets the running
operation finish and leaves the rest `cancelled`, retaining completed changes
rather than rolling them back. Undo is whole-batch: if any file no longer matches
what Folio saved, nothing is undone and the blocking file is named. A restart
restores previews without their approvals and resumes no mutation.

The digest deliberately covers only what can change a file. Ripple candidates are
review evidence that never writes, so adding or removing one does not invalidate
an approval the user already gave.

The trade-off is that these rules exist in both languages before the writer does.
They are kept compiled and tested on both sides rather than waiting in a branch,
and `apply_plan` refuses with `writerNotImplemented` so no save is ever reported
that the filesystem did not make. The writer itself is issue #5.
