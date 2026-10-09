# Folio

Folio is a private file workspace for finding, organizing, and understanding local documents. Its language distinguishes document evidence from suggestions and approved changes.

## Language

**SOS**: Folio's three primary capabilities: Search, Organize, and Summarize.
_Avoid_: A replacement name for the assistant or the shared AI pipeline.

**Workspace**: The set of local folders the user has authorized Folio to access. In the MVP, a workspace is one active authorized folder at a time.
_Avoid_: Entire device, cloud drive.

**Document**: An original file in an authorized folder that Folio can identify and inspect.
_Avoid_: A cached copy or a generated summary.

**Source Passage**: A located excerpt from a document supporting a result or claim.
_Avoid_: Unattributed context.

**Search Result**: A document match presented with its location and relevant source passages.
_Avoid_: A generated answer without matching files.

**File Summary**: A concise account of one document's contents, with references to its supporting passages.
_Avoid_: Unattributed interpretation.

**Partial Summary**: A summary of only part of a document, with the covered sections or passages identified.
_Avoid_: A whole-file summary or an implication that unread sections were covered.

**Project Summary**: A summary of an explicitly identified group of documents, with its coverage stated.
_Avoid_: A claim to cover every file when only retrieved excerpts were inspected.

**Relationship**: An evidenced connection between two documents with a stated type and provenance.
_Avoid_: Dependency as a generic name for all connections.

**Similarity**: A connection indicating related subject matter.
_Avoid_: Proof that changing one document affects the other.

**Explicit Reference**: A passage in one document that names or links another document.
_Avoid_: An inferred dependency.

**Shared Fact Candidate**: Passages in separate documents that may refer to the same fact, subject, or event.
_Avoid_: A confirmed contradiction without comparison evidence.

**Relationship Graph**: A view of documents and their typed, evidenced connections.
_Avoid_: A decorative graph without inspectable evidence.

**Virtual Collection**: A named group of document references that preserves original file locations.
_Avoid_: Folder, duplicate storage.

**Organization Suggestion**: A proposed grouping, filename, or destination awaiting review when it changes physical files.
_Avoid_: An already completed move.

**Action Plan**: The exact file operations and effects proposed in response to a request.
_Avoid_: Execution, completed action.

**Approval**: The user's authorization of a particular current action plan.
_Avoid_: Authorization of future or changed plans.

**Folio Ripple**: The impact review produced for a proposed edit, identifying related documents and supporting passages that may need attention.
_Avoid_: Automatic propagation or a guarantee of comprehensive impact coverage.

**History Entry**: A record of an applied file change, recoverable until Folio stops keeping the content needed to reverse it.
_Avoid_: A record of an unsaved preview.

**Undo**: A user-requested reversal of an applied change after checking the current file state.
_Avoid_: Overwriting unrelated external edits.

**Local Sync**: Refreshing Folio's local understanding after files change.
_Avoid_: Cross-device or cloud synchronization.

**Stale Document**: A document that changed but could not be re-read, so Folio still shows what it knew from the previous version.
_Avoid_: A current search result, a deleted document.

**Check Again**: Asking Folio to read a document that could not be read, without waiting for Local Sync's next attempt.
_Avoid_: Treating a document Folio cannot read yet as permanently unreadable.

**Model Lab**: The settings area for choosing local models and inspecting actual task measurements.
_Avoid_: Fabricated benchmarks or the main product identity.

**Workspace Identity**: The stable identifier of an authorized folder, unchanged across restarts of Folio.
_Avoid_: A new identifier each time the same folder is chosen.

**Document Identity**: The stable identifier of a document inside a workspace, unchanged by editing its contents.
_Avoid_: A bare file path, or an identifier derived from the document's text.

**Source Offset**: The position of a passage inside a document, counted in UTF-8 bytes of its text.
_Avoid_: A character count, a UTF-16 index, or an offset without a stated unit.

**Document Revision**: The content hash a passage, relationship or operation was derived from.
_Avoid_: Evidence presented without the version of the file it came from.

**Embedding Space**: The model revision, quantization, dimensions and preprocessing a set of vectors was produced with.
_Avoid_: Comparing vectors from two spaces, or treating equal dimensions as the same space.

**Plan Digest**: The fingerprint of an action plan's exact operations, to which an approval is bound.
_Avoid_: Approval of a plan identifier whose operations have since changed.

**Preflight**: The check of every target in a plan before any file is changed.
_Avoid_: Discovering an unusable target midway through a batch.

**Operation Outcome**: The durable record of one operation as succeeded, failed, cancelled or not started.
_Avoid_: A single result for a whole batch, or a reported save the filesystem did not make.

**Batch**: The ordered operations of one approved plan, applied together and reported per operation.
_Avoid_: A promise that every file changes or none does.

**Undo Conflict**: A file that no longer matches what Folio saved, which stops the whole Undo.
_Avoid_: Reversing part of a batch, or discarding a newer external edit.

**Activity**: The record of changes Folio actually made to files, from its native history, one entry per approved plan, with Undo when it is safe.
_Avoid_: Listing previews, suggestions, summaries or analyses as changes.

**Olio**: Folio's mascot and the name of its assistant in Ask & Act.
_Avoid_: Attributing an answer to Olio that no retrieved passage supports.

**Recent Files**: Files opened in Folio on this device, newest first.
_Avoid_: Recently modified files, or activity Folio didn't observe.

**Pinned Folder**: A folder inside the workspace that the user keeps as a quick filter on Home, remembered on this device.
_Avoid_: A permission, or a folder outside the workspace.
