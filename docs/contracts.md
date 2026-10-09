# Frozen cross-track contracts

This is the agreed boundary between the UI, the native core and the provider
adapters. `src/domain/contracts.ts` and `src-tauri/src/contracts.rs` declare the
same shapes; `fixtures/contracts/contract-cases.json` pins the encodings that
both languages must produce.

Change this file, both declarations and the fixtures together, and tell the
other owners before merging. TJ coordinates contract changes.

## Failures

Every failure crossing the boundary is `{ code, message, details? }`. Native
commands return it from `Result::Err`, so the UI receives a code instead of a
sentence to parse. `code` is one of `FolioErrorCode` / `ErrorCode`; `message` is
English prose for the user; `details` is a flat map of strings for context such
as `path`, `planId` or `blockingRelativePath`.

Callers branch on `code`, never on `message`. The fixture file carries the
complete code list in wire order.

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

## Plans, approval and outcomes

An `ActionPlan` is issued by the native core with an identity, a workspace, a
validity window, ordered operations, Ripple `impacts` and a `digest`.

**Canonical bytes.** `FOLIO-PLAN-V1`, then every field as
`<utf8ByteLength>:<value>\n`: plan id, workspace id, `createdAt`, `expiresAt`,
operation count, then per operation its kind followed by its fields in a fixed
order. Length prefixes mean no path or document body can forge a field
boundary. `digest` is `sha256` over those bytes.

The digest covers exactly what can change a file. `impacts` are review
candidates that never write, so they are excluded and cannot silently
invalidate an approval.

**Approval** binds to one plan identity _and_ its digest. The native registry
accepts an approval only for a plan it issued, and only when the caller echoes
the digest it was shown. A UI-generated plan identity is `planUnknown`; a
UI-generated digest is `planDigestMismatch`; a plan whose operations changed is
`approvalStale`.

**Preflight** checks every operation before any file changes: containment,
supported media type, duplicate targets within the batch, the expected content
hash of each target, and the absence of every destination. `expectedDestination:
"absent"` is explicit, so a rename never overwrites an existing file.

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
nothing is rolled back automatically.

**Undo** is whole-batch. Every applied entry is checked against the current file
state first. If anything conflicts — `externallyModified`, `missing`,
`destinationOccupied` or `notRecoverable` — no file changes and the blocking
file is named through `undoConflict` details. Newer external edits survive.

**Restart** restores explicitly selected folders and unfinished previews,
revalidates folder access, drops every approval and resumes no mutation.
`RestoredPreview.requiresFreshApproval` is always true.

## What is not implemented yet

The native writer, history persistence and real Undo are
[issue #5](https://github.com/duckycodess/folio/issues/5). `apply_plan` refuses
with `writerNotImplemented` rather than reporting a save that never happened.
Local embedding and generation are issues #4 and #8. The pending cases are
listed, not mocked, in `src/domain/pending.test.ts` and as ignored tests in
`src-tauri/src/plan.rs`.
