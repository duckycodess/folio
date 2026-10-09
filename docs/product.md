# Product scope

## Vision

Folio is a privacy-focused desktop application that helps people search, organize, and summarize their files entirely on-device. Students are the first audience, while folders and operations remain general-purpose. Semantic search, a relationship graph, contextual summaries, approved natural-language file operations, and recoverable history make scattered documents easier to find and maintain.

The primary areas are **Search, Organize, Summarize**. The file explorer, relationship graph, and assistant connect these areas; each also supports direct interaction.

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

## Boundaries

TXT and Markdown support content editing. Text-based PDFs support reading and indexing; PDF content editing and scanned-PDF OCR are later work. Natural-language deletion is outside the agreed hackathon delivery.

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
