# Product scope

<!-- impeccable:product-schema 1 -->

## Platform

web

Folio is delivered as a Tauri desktop application for Windows and macOS. Here, `web` identifies the React webview interface and its browser development preview; Windows and macOS remain the product's delivery targets. Phones are later scope.

## Users

Students are the first audience. Their initial tasks include finding coursework, notes and project documents, understanding source material, organizing scattered files, and reviewing a change that may affect other documents. General-purpose folder and file workflows remain available to other users.

English, Filipino and Taglish documents, requests and responses are core coverage. Initial UI labels may be English. Do not advertise additional languages as equally evaluated without evidence.

## Vision

Folio is a privacy-focused desktop application that helps people search, organize, and summarize their files entirely on-device. Students are the first audience, while folders and operations remain general-purpose. Semantic search, a relationship graph, contextual summaries, approved natural-language file operations, and recoverable history make scattered documents easier to find and maintain.

The primary areas are **Search, Organize, Summarize**. The file explorer, relationship graph, and assistant connect these areas; each also supports direct interaction.

## Product Purpose

Help a user find the right local document and supporting passage, understand its contents and connections, and manage an exact file change with preview, approval and recoverable history. Success means completing Search, Organize and Summarize directly, as well as the supporting Ask & Act workflow, with internet disabled after setup.

The full MVP is required. The user authorized replacing the current starter UI; this changes the presentation rather than the settled product scope. Current capabilities and unfinished implementation remain recorded in [implementation status](status.md).

## Positioning

Folio combines multilingual document retrieval, source-grounded understanding and approved local file operations in one private workspace. Its source passages, typed relationships and Folio Ripple review connect a proposed change to evidence in the user's own files. Original documents remain in authorized folders, and related passages become review candidates rather than automatic edits.

This is the intended product mechanism, not a claim that the starter already implements it or that competing products lack these features.

## Operating Context

Users select local folders through the desktop application's native picker. They read original documents, compare source evidence, inspect proposed filenames or content changes, and approve an exact plan before files change. Core operations run on-device after explicit model setup; initial downloads may require internet.

The independent journeys are:

| Journey                  | Steps                                                                                             |
| ------------------------ | ------------------------------------------------------------------------------------------------- |
| A — Explore & Understand | Home → Browse or Search Files → Select File → View Graph & Summary → Explore Related Files        |
| B — Smart Organize       | Organize → Select Collection or Folder → Analyze → Preview Suggested Changes → Approve & Apply    |
| C — Ask & Act            | AI Assistant → Enter Instruction → Find Target Files → Preview Actions & Impacts → Approve & Save |

The shared pipeline is request → find files → understand content → discover related files → edit and analyze impact → approve, save and refresh the local index. The graph and assistant support SOS instead of replacing its independent entry points. See [workflows](workflows.md).

The prepared demo uses 10–20 local documents with known repeated facts. Its central Taglish request is “Hanapin yung project plan at palitan ang deadline na October 20 to October 23.” The user checks the target and exact diff, reviews related old-deadline passages, approves a real save, sees local refresh, and can undo if the current file still matches the applied change.

## Required hackathon scope

| Area            | Acceptance behavior                                                                                                                               |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Workspace       | User selects authorized local folders; documents retain their original locations.                                                                 |
| Search          | Natural-language retrieval finds relevant documents and displays paths and source passages. English, Filipino, and Taglish requests are included. |
| Understand      | File questions and individual summaries use document passages and cite their sources.                                                             |
| Relationships   | Display similarity, explicit references, and shared fact candidates separately; users can inspect the evidence.                                   |
| Organize        | Detect byte-identical duplicates, suggest meaningful filenames, and preview rename/move actions before approval.                                  |
| Create and edit | Create TXT/Markdown files and propose TXT/Markdown content edits, with a preview and approval before writing.                                     |
| Ripple          | Before an edit is approved, show related passages that may need review. Save only the approved target; retain the review list.                    |
| History         | Applied changes have recoverable history; undo checks for external modifications.                                                                 |
| Local sync      | Refresh text, retrieval data, affected connections, and cached summaries after a successful change.                                               |
| Model Lab       | Select an installed local model and compare real task time, labelled task correctness, and model disk size on a small fixed suite.                |

## Capabilities and Constraints

The required scope describes MVP outcomes; it is not a list of completed capabilities. Native commands own folder permissions and authoritative plans, approvals, writes and history. The UI consumes narrow adapters and never receives unrestricted filesystem access. Retrieved document text is evidence and cannot authorize an operation.

Search results show document locations and matching source passages. Summaries and answers cite supporting passages; insufficient evidence and ambiguous target identity remain explicit. Distinguish similarity, explicit references and shared fact candidates, with inspectable provenance. Folio Ripple flags evidence for review and does not promise comprehensive impact detection.

Keep generation and embeddings behind separate adapters. Different embedding revisions have separate or rebuilt indexes. One active generative model/request is the initial limit, with bounded context, caches and history, progress and cancellation. Model Lab is a small settings and fixed-task comparison area; it reports actual task outcomes and measurement conditions.

## Boundaries

TXT and Markdown support content editing. Text-based PDFs support reading and indexing; PDF content editing and scanned-PDF OCR are later work. Natural-language deletion is outside the agreed hackathon delivery. Deleting one TXT or Markdown file from the Graph is a manual action with an exact preview, approval and Undo; Folio keeps the file's contents in history and lists the files whose links will break without changing them (ADR 0010).

The demo uses 10–20 prepared documents with known related facts. The starter includes 15 text fixtures; PDF parser acceptance requires adding and testing text-based PDF fixtures.

The UI initially uses English labels. Documents, search, commands, and model responses support English, Filipino, and Taglish; respond in the user's language unless asked otherwise. Include cross-language retrieval, not just same-language matching. Additional languages remain available through multilingual models, with tested coverage reported honestly.

Core operations must complete with internet disabled after installation/model setup. Initial model downloads can require internet. Hosted Jev and MCP remain optional future adapters. Sync is local index/graph refresh; cloud and cross-device synchronization are outside this MVP.

Automatic virtual collections and project-wide summaries are stretch goals. Virtual collections do not copy or move documents; physical operations always require approval. Exact duplicate detection does not imply that similar documents are duplicates.

## Engineering targets

- Windows and macOS desktop delivery; phones follow later.
- An 8 GB laptop without a dedicated GPU is the minimum test target; do not claim smoothness before measurements.
- Default app, runtime, tokenizer, and model files target under 1 GB installed. Original documents, variable indexes, and bounded history are accounted for separately. Optional model packs can have a separately displayed size.
- Normal browsing and search remain responsive while inference runs. Generation has progress and cancellation.
- One active generative model/request initially. Models load on demand and unload when idle.

These are design targets. A skeleton or a model download size does not establish measured installation or memory usage.

## Brand Commitments

The product name is **Folio**. **Search, Organize, Summarize (SOS)** remain the primary capabilities. Use the definitions in the [glossary](../GLOSSARY.md), including Source Passage, Action Plan, Approval, Folio Ripple, Undo and Local Sync. User-facing language distinguishes evidence, suggestions, previews and completed saves, and gives understandable setup or recovery steps.

The current UI is authorized for replacement. Its existing visual styling is not a newly confirmed design requirement. This product record establishes no replacement palette, typography or visual world.

## Evidence on Hand

- [The settled interview](grill-with-docs.md) records the original audience, platforms, formats, offline behaviour, languages, workflows and MVP boundaries. The current user confirmed this scope and requested the full MVP with a replacement UI.
- [Fifteen synthetic documents](../fixtures/documents) provide English, Filipino and Taglish examples. No real user documents, model weights, local databases or private edit history belong in Git.
- [Labelled benchmark cases](../fixtures/benchmark-cases.json) cover retrieval, a sourced Filipino summary, a Taglish action and the project-deadline Ripple case. They are expected outcomes, not completed model results.
- [The acceptance matrix](acceptance.md) defines offline and cross-language coverage, insufficient evidence, ambiguity, approval, containment, collisions, failures, undo conflicts and embedding-space isolation.
- [Shared contracts](../src/domain/contracts.ts), deterministic tests, scoped native TXT/Markdown commands, and a [SQLite migration](../src-tauri/migrations/001_initial.sql) form the starter implementation. A migration contract does not prove a connected persistent index.
- [The original workflow image](assets/workflow-reference.png) documents journeys A/B/C. Existing native icons are in `src-tauri/icons/`; they are assets on hand rather than a commitment to preserve the current interface.
- [Architecture decisions](adr), [setup](setup.md) and [implementation status](status.md) distinguish accepted boundaries from runtime candidates and missing verification.
- [The MVP index](https://github.com/duckycodess/folio/issues/1) links the named team's implementation and checking tickets. TJ leads verification/integration, Dann owns AI, Gab owns native workspace/actions, and Louise owns UI. The [team plan](plan.md) carries the responsibilities.

No model-correctness, installed-size, memory, performance or consumer-package result may be invented from a model card, synthetic example or passing frontend build.

## Product Principles

1. **Evidence before claims.** Give users the document identity, location and supporting passage behind retrieval, answers, relationships and impact review.
2. **Exact approval and recoverability.** Preview physical changes, bind approval to the current plan, and preserve unrelated external edits during failures and undo.
3. **Private local operation.** Core work stays on-device and works offline after setup; optional hosted integrations are not core dependencies.
4. **Multilingual as a core workflow.** English, Filipino and Taglish matter in requests, documents and responses, including cross-language retrieval.
5. **Useful SOS workflows with honest state.** Keep direct Search, Organize and Summarize paths, show real capability readiness, and distinguish previews from completed work.

## Accessibility & Inclusion

The replacement UI must support keyboard-only workflows, visible focus, programmatic active/selected state, accessible loading/status/error announcements, and correct modal focus/close/return behaviour. Normal text meets 4.5:1 contrast; essential controls and focus indicators meet applicable non-text contrast requirements. These are acceptance requirements, not a claim that the current starter is compliant.

At desktop window sizes and 200% zoom, long English, Filipino and Taglish filenames, source passages and paths remain readable, and workspace selection and primary actions remain reachable. Respect the application's configured minimum window size without treating mobile packaging as part of this MVP. TJ independently checks accessibility and failure recovery; Louise owns the UI changes.

## Open Decisions and Verification

No MVP scope decision is reopened by this init. Model selection, quantized multilingual task quality, reliable PDF extraction, native packaging, installed size, memory and responsiveness remain implementation or measurement items until verified. The model candidates in [architecture](architecture.md) are candidates rather than benchmarked recommendations. New consequential architecture trade-offs belong in the existing ADR sequence.
