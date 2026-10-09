# Folio

Folio is a private file workspace for finding, organizing, and understanding local documents. Its language distinguishes document evidence from suggestions and approved changes.

## Language

**SOS**: Folio's three primary capabilities: Search, Organize, and Summarize.
_Avoid_: A replacement name for the assistant or the shared AI pipeline.

**Workspace**: The set of local folders the user has authorized Folio to access.
_Avoid_: Entire device, cloud drive.

**Document**: An original file in an authorized folder that Folio can identify and inspect.
_Avoid_: A cached copy or a generated summary.

**Source Passage**: A located excerpt from a document supporting a result or claim.
_Avoid_: Unattributed context.

**Search Result**: A document match presented with its location and relevant source passages.
_Avoid_: A generated answer without matching files.

**File Summary**: A concise account of one document's contents, with references to its supporting passages.
_Avoid_: Unattributed interpretation.

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

**History Entry**: A recoverable record of an applied file change.
_Avoid_: A record of an unsaved preview.

**Undo**: A user-requested reversal of an applied change after checking the current file state.
_Avoid_: Overwriting unrelated external edits.

**Local Sync**: Refreshing Folio's local understanding after files change.
_Avoid_: Cross-device or cloud synchronization.

**Model Lab**: The settings area for choosing local models and inspecting actual task measurements.
_Avoid_: Fabricated benchmarks or the main product identity.
