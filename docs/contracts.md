# Frozen cross-track contracts

This is the agreed boundary between the UI, the native core and the provider
adapters. `src/domain/contracts.ts` and `src-tauri/src/contracts.rs` declare the
same shapes; `fixtures/contracts/contract-cases.json` pins the encodings that
both languages must produce. That file is produced by
`fixtures/contracts/generate-contract-cases.py`, a third implementation written
from this document, so the two languages are checked against the rules rather
than against each other.

Change this file, both declarations and the fixtures together, and tell the
other owners before merging. TJ coordinates contract changes.

## Failures

Every failure crossing the boundary is `{ code, message, details? }`. Native
commands return it from `Result::Err`, so the UI receives a code instead of a
sentence to parse. `code` is one of `FolioErrorCode` / `ErrorCode`; `message` is
English prose for the user; `details` is a flat map of strings for context such
as `path`, `planId` or `blockingRelativePath`. Both sides carry details as
strings — `Record<string, string>` and `BTreeMap<String, String>` — so numbers
and absent values are stringified where they are reported rather than crossing
the boundary in two different shapes.

Callers branch on `code`, never on `message`. The fixture file carries the
complete code list in wire order. A code this build does not know becomes
`internal`, keeping the reported one under `details.reportedCode`, so a newer
native core can never have an unrecognized failure treated as a specific,
actionable one.

## Identity

**Workspace.** `WorkspaceId` is derived from the canonical root path, so the
same folder keeps its identity across restarts and a restored preview still
points at the folder it came from. It contains no `:`.

**Document.** `DocumentId` is `${workspaceId}:${relativePath}`. It is reversible
and never lossy, and it does not change when a document is edited.

**Relative path.** A `/`-separated, NFC-normalized path below an authorized
root. Absolute paths, `\`, `.`, `..`, empty segments and control characters are
refused rather than repaired, because a repaired path identifies a different
file. A filename the operating system reports as invalid Unicode is refused with
`pathUnsupportedEncoding`; `list_documents` reports it under `skipped` instead of
inventing a name for it.

Which of these a given system can actually produce differs: macOS refuses to
create a filename that is not valid UTF-8, and on Windows a backslash is the
path separator rather than a filename character. The refusals are pure string
logic and are tested on every platform; the fixtures that need such a file on
disk are scoped to the systems that can hold one.

macOS reports decomposed filenames and Windows composed ones, so normalization
happens before an identity is formed. A destination Folio would create is also
checked for names Windows cannot store (`assertPortableDestination`).

## Content hashes and sizes

`ContentHash` is `sha256:<64 lowercase hex>` over the exact bytes of a file or
payload. `DocumentRecord.sizeBytes` is the exact byte length on disk — never a
character count and never a UTF-16 length.

## Source offsets

`SourcePassage.offsetUnit` is `utf8Byte`, the only unit on the boundary. `start`
and `end` are UTF-8 byte offsets on character boundaries, half-open. `page` is a
1-based page number for paged media. `documentContentHash` records the revision
the offsets refer to, so evidence from an earlier revision is detectable.

JavaScript string indexes are converted at the edge (`src/domain/offsets.ts`).
The unit is a union of one so that adding another is a visible contract change.

## Relationships

A relationship is a discriminated union carrying evidence typed for its kind:

- `explicitReference` — `documentLink` provenance, the raw and resolved link,
  and at least one passage in the source document.
- `similarity` — `embedding` provenance, the `spaceFingerprint` it was computed
  in, a score in [0, 1], and passages in both documents. It is never a claim
  that an edit must propagate.
- `sharedFactCandidate` — passages in both documents and an optional confidence.
  It is never a confirmed contradiction.

A Ripple `ImpactCandidate` may carry the `relationshipType` and `provenance` of
the relationship that connected it to the edited document. Both are optional
and absent for a byte-identical copy, which is related by content alone. A
`sharedFactCandidate` that mentions the replaced value is `evidence`; a
`similarity` relationship is only ever `similarityOnly`. (Added with issue #5;
older payloads without these fields remain valid.)

For a `delete`, the candidates are what the deletion leaves for review (ADR
0010): a document that links to the deleted file is `evidence`
(`explicitReference`, `documentLink`) with the link passages located in that
document, because the link will stop working; a `sharedFactCandidate` is
`evidence` with its stored provenance and its own passages; a `similarity`
relationship and a byte-identical copy are `similarityOnly`, the copy without a
relationship type. A document the deleted file only links to is not listed.
Folio changes none of them.

Vectors are compared only within one embedding space, identified by
`folio-space-v1/<modelId>/<revision>/<quantization>/<dimensions>/<preprocessing>`
with `%` and `/` escaped.

## Providers

Embedding and generation stay behind separate interfaces. An adapter rejects
with `modelNotInstalled`, `modelLoadFailed`, `providerBusy`, `cancelled` or
`contextOverflow`. Aborting the request's `signal` rejects with `cancelled`.
One generative request runs at a time; a second concurrent request is
`providerBusy`. A run either returns an answer or reports
`insufficientEvidence` — it does not invent one.

## Local AI provider results (issue #4)

The local AI boundary adds result types without changing the frozen #2
`GroundedAnswer` contract. `GroundedAnswer` contains `text`, `sources`, the
covered `DocumentId[]`, `modelId`, and required provider `revision`.
`GroundedResult` extends that shape with the required answer kind, sentence
citations, `coverageRanges` carrying content hashes and UTF-8 byte ranges, and
`uncitedSentenceCount`. The covered document IDs are distinct from the ranges:
they describe retrieved evidence, not a claim about the whole corpus. A
no-evidence result uses `revision: "none"` only when no provider ran.

The provider adapter keeps embedding and generation spaces separate. Semantic
results carry `spaceFingerprint`, and a query or cached index from another
model revision, quantization, dimension or preprocessing fingerprint is
rejected rather than compared. The native registry remains the only workspace
authority; AI commands resolve the registered root before reading files.

For issue #4's additive proposal boundary, edit, rename and move proposals
carry `relativePath` and the observed file `observedContentHash` alongside the
native `documentId`. These fields are evidence for a later native plan; they
are not approval, an action plan, or permission to write. The native plan and
approval engine re-check the current file before any future mutation.

Core provider failures are translated at the native boundary to the frozen
`FolioError` wire shape:

| Core failure                                  | Wire error               | Details                                               |
| --------------------------------------------- | ------------------------ | ----------------------------------------------------- |
| `modelNotInstalled`                           | `modelNotInstalled`      | `modelId`                                             |
| `runtimeMissing`                              | `modelNotInstalled`      | `component: "runtime"`, `runtimeId`                   |
| `modelCorrupt`                                | `modelLoadFailed`        | `reason: "verificationFailed"`                        |
| `runtimeStartFailed`                          | `modelLoadFailed`        | `reason: "runtimeStartFailed"`                        |
| `generationBusy`                              | `providerBusy`           | —                                                     |
| `cancelled`                                   | `cancelled`              | —                                                     |
| `contextLimit`                                | `contextOverflow`        | —                                                     |
| `embeddingSpaceMismatch`                      | `embeddingSpaceMismatch` | `expected`, `actual`                                  |
| `invalidModelOutput`, `noEvidence`, `ioError` | `internal`               | `reportedCode` plus a safe digest/path when available |

These mappings and the additive result/proposal types are issue #4 proposals
for TJ review. They do not add or weaken frozen error enums or identity types;
unknown wire errors remain `internal` with their reported code in details.

## Plans, approval and outcomes

An `ActionPlan` is issued by the native core with an identity, a workspace, a
validity window, ordered operations, Ripple `impacts` and a `digest`.

**Canonical bytes.** `FOLIO-PLAN-V1`, then every field as
`<utf8ByteLength>:<value>\n`: plan id, workspace id, `createdAt`, `expiresAt`,
operation count, then per operation its kind followed by its fields in a fixed
order: `create` — destination path, media type, content; `edit` — document id,
path, expected hash, new content; `rename` and `move` — document id, path,
expected hash, destination path; `delete` — document id, path, expected hash.
Length prefixes mean no path or document body can forge a field boundary.
`digest` is `sha256` over those bytes.

The digest covers exactly what can change a file. `impacts` are review
candidates that never write, so they are excluded and cannot silently
invalidate an approval.

**Approval** binds to one plan identity _and_ its digest. The native registry
accepts an approval only for a plan it issued, and only when the caller echoes
the digest it was shown. A UI-generated plan identity is `planUnknown`; a
UI-generated digest is `planDigestMismatch`; a plan whose operations changed is
`approvalStale`.

**Preflight** checks every operation before any file changes, in two passes.
The whole batch is validated structurally first — containment, supported media
type, and duplicate targets within the batch — so a plan that can never be valid
is refused the same way whatever the current files happen to be. Only then is it
compared against the filesystem: the expected content hash of each target, that
each target is a file, and the absence of every destination.
`expectedDestination: "absent"` is explicit, so a rename never overwrites an
existing file.

Duplicate detection compares paths case-insensitively, because Windows and macOS
folders are usually case-insensitive and two operations naming the same file in
different cases are one file in practice. A rename whose destination differs
from its source only in case is refused for the same reason.

**Durable outcomes.** A batch records one `OperationOutcome` per operation:

| Status       | Meaning                                                       |
| ------------ | ------------------------------------------------------------- |
| `succeeded`  | The file changed; `historyEntryId` records how to reverse it. |
| `failed`     | This operation stopped the batch; earlier successes are kept. |
| `cancelled`  | Not started because the user cancelled after saving began.    |
| `notStarted` | Not started because an earlier operation failed.              |

`BatchResult.stopReason` is `completed`, `failed` or `cancelled`. A failure in
the final attempt wins over a pending cancellation, because the failure is what
stopped the batch. No universal filesystem atomicity is promised.

**Cancellation** lets the running operation finish and records its outcome, then
stops before the next one. Completed changes are retained and offered to Undo;
nothing is rolled back automatically. A cancellation reported with no attempts
is refused on both sides: nothing began, so there is no outcome to record.

**Undo** is whole-batch. Every applied entry is checked against the current file
state first. If anything conflicts — `externallyModified`, `missing`,
`destinationOccupied` or `notRecoverable` — no file changes and the blocking
file is named through `undoConflict` details. Newer external edits survive.

**Restart** restores explicitly selected folders and unfinished previews,
revalidates folder access, drops every approval and resumes no mutation.
`RestoredPreview.requiresFreshApproval` is always true.

**Undo needs a confirmed preview.** `preview_undo` returns the
`UndoPreflight` for a plan without writing. `undo_plan` takes the `entryIds` of
the preview the user confirmed; if the pending entries are no longer exactly
those, it refuses with `approvalStale` and changes nothing. An Undo that stops
partway keeps what it reversed and leaves the rest pending, so a fresh preview
can finish it.

**Deletion** (issue #44, ADR 0010). A `delete` names `documentId`,
`relativePath` and `expectedContentHash` and has no destination. Like an edit,
it applies only to TXT and Markdown files; a PDF is `unsupportedMediaType`. The
writer reads the file, checks its hash, stores its exact bytes in history and
only then removes it. If the bytes can't be stored, nothing is deleted
(`internal`). If the file changed after the preview, or changes or can't be
removed while Folio deletes it, the stored entry is dropped and the file is
kept (`targetChanged`, or `documentUnavailable` from the filesystem). Undo
re-creates the file with an exclusive create at its previous path; a file now
using that name is `destinationOccupied` and is never replaced, and a folder
that is gone is `missing`.

**History.** Each `HistoryEntry` carries its `operationKind`. An entry for a
`delete` has a `beforeRelativePath` and `beforeContentHash` but no
`afterRelativePath` or `afterContentHash`, because nothing exists after it.

**The writer** (`src-tauri/src/writer.rs`, issue #5) applies an approved plan
and returns `{ batch: BatchResult, historySettled, indexRefreshed }`. An error
from `apply_plan` means no file changed; once any operation has run, the report
is always returned, and `historySettled: false` says that bookkeeping after the
writes failed, not that the writes did. A rename or move never replaces an
existing file. Every operation re-checks its source's hash immediately before
running, and an edit checks it again just before swapping in the new content.
An edit keeps the file's permissions, and a file Folio may not write (read-only,
or owned by someone else) is refused rather than replaced. Edits and deletions
keep the content Undo needs for the 100 most recent applied plans; older edit
and delete entries remain listed with `recoverable: false`.

## What is not implemented yet

Local embedding and generation are issues #4 and #8, including model-generated
Ripple explanations. The pending cases are listed, not mocked, in
`src/domain/pending.test.ts`.
