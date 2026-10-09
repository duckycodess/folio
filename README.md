# Folio

**Search. Organize. Summarize.** A private desktop workspace for understanding and managing local documents in English, Filipino, and Taglish.

Folio finds documents by meaning, explains their connections, proposes file changes, and shows which related documents may need review. Documents and inference stay on the device after setup. Students are the first audience; the file workflows remain useful to everyone.

## Starter status

This is a development starter, not a finished Folio release.

| Available in this starter                                                        | Still to implement                                                               |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| React/TypeScript SOS interface and all three entry workflows                     | Multilingual semantic retrieval (keyword index is native and persistent)         |
| Fifteen synthetic English/Filipino/Taglish documents                             | Local model downloads, loading, and generation                                   |
| Keyword filtering and explicit Markdown-reference discovery                      | AI summaries, natural-language action interpretation, semantic graph edges       |
| Tauri commands for folders, a persistent index, and TXT/Markdown/text-PDF reads  | UI wiring for the native index, actions and Undo                                 |
| Frozen contracts, native approved apply/undo with history, and Ripple evidence   | Model-generated Ripple explanations; real-model Model Lab runs (manual workflow) |
| Glossary, decision records, acceptance criteria, team plan, and CI configuration | Tested Windows/macOS packages and measured installation/RAM budgets              |

The browser preview uses synthetic fixtures and cannot modify your filesystem. It labels keyword search and missing models explicitly. No model output, semantic relationship, benchmark result, or successful save is fabricated.

## Quick start

Use Node.js 22.12+ or a compatible supported newer release, and npm.

```bash
npm ci
npm run dev
```

Open the local address printed by Vite. For desktop development, first install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, including Rust, then run:

```bash
npm run tauri dev
```

Build the Windows application on Windows and the macOS application on macOS. WSL can run the frontend and core checks; it is not a substitute for verifying a Windows installer. The starter does not include model weights or inference binaries. See [setup](docs/setup.md).

```bash
npm run check
npm test
npm run build
```

## Product workflows

| Workflow                 | Journey                                                                                           |
| ------------------------ | ------------------------------------------------------------------------------------------------- |
| A — Explore & Understand | Home → Files or Search → Select File → View Graph & Summary → Explore Related Files               |
| B — Smart Organize       | Organize → Select Collection or Folder → Analyze → Preview Suggested Changes → Approve & Apply    |
| C — Ask & Act            | AI Assistant → Enter Instruction → Find Target Files → Preview Actions & Impacts → Approve & Save |

Search, Organize, and Summarize remain primary areas. The graph and assistant support those areas. The shared AI pipeline is **request → find files → understand content → discover related files → edit and analyze impact → approve, save, and refresh the local index**.

## Start contributing

Read [AGENTS.md](AGENTS.md), [GLOSSARY.md](GLOSSARY.md), [product scope](docs/product.md), and the [four-person implementation plan](docs/plan.md). Each track should implement its shared contracts before changing another track's code.

The design session is recorded in [grill-with-docs](docs/grill-with-docs.md). New product decisions should update these documents; new consequential architecture trade-offs belong in [docs/adr](docs/adr).

The repository contains only synthetic examples. Keep real documents, model files, local databases, secrets, and edit history outside Git.

## License

MIT. Model weights and third-party runtimes retain their own licenses and notices.
