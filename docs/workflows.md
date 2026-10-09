# SOS workflows

The reference image defines three user journeys. Preserve their independent entry points.

![Original team workflow reference](assets/workflow-reference.png)

| Journey                  | Steps                                                                                             | Purpose                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| A — Explore & Understand | Home → Browse or Search Files → Select File → View Graph & Summary → Explore Related Files        | Locate documents, understand contents, discover connections and locations.           |
| B — Smart Organize       | Organize → Select Collection or Folder → Analyze → Preview Suggested Changes → Approve & Apply    | Group virtually; suggest names and destinations; identify exact duplicates.          |
| C — Ask & Act            | AI Assistant → Enter Instruction → Find Target Files → Preview Actions & Impacts → Approve & Save | Natural-language create/read/edit/rename/move, grounded questions, and Folio Ripple. |

## Shared request pipeline

1. Receive the request, including English, Filipino, or Taglish.
2. Find target documents in authorized folders. Present choices if identity is ambiguous.
3. Understand relevant contents using located passages.
4. Discover related documents through candidate retrieval, explicit links, and graph traversal.
5. Prepare the edit/action preview and Ripple evidence. Treat findings as review candidates.
6. Ask approval for the exact plan. Reject a stale plan if a target changed.
7. Apply the approved operations, record history, and refresh the affected local index and graph.

## Concrete demonstration

Request: **“Hanapin yung project plan at palitan ang deadline na October 20 to October 23.”**

Folio finds `project-plan.md`, shows the supporting deadline passage, and discovers `meeting-notes.md` and `submission-checklist.md`. The preview changes only the target plan. Ripple flags old-deadline passages in the two other files. After approval, the target is saved and its local index is refreshed. The two review candidates remain unchanged until separately reviewed. Undo restores the target only if its current state still matches the applied change.

## Summarize remains core

Selecting a file offers a source-grounded individual summary. The assistant can request the same action with **“Ibuod itong document.”** Saving a generated summary as a new document is a separate approved create operation. Project-wide summaries are a stretch goal with explicit coverage and source references.

## Failure behavior

No model: browsing, keyword search, and native operations remain available; AI-dependent actions show setup guidance. No matching evidence: show the lack of evidence rather than inventing an answer. Ambiguous files: ask the user to choose. Permission loss, rename collision, external edit, or failed save: show the error and retain the preview without claiming success.
