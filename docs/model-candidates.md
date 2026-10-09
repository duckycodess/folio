# Optional model candidates (evaluation-only)

Research for issue #8, 2026-10-10. **These are evaluation-only candidates, not supported or recommended models.** They are measured by Model Lab and become selectable only after a review of required task outcomes and resource measurements (`docs/grill-with-docs.md`, optional-model expansion interview).

- The default models and the under-1-GB default-install target are unchanged.
- Nothing here was downloaded or run. File facts come from the Hugging Face API (`/api/models/<repo>?blobs=true`, LFS `sha256`) on the date above. Language and evaluation statements come from each model's own card.
- **Weights, runtime RAM, speed and Filipino/Taglish task quality are unmeasured.**
- **Catalog.** The pins are in `src-tauri/resources/model-evaluation-candidates.json`, a catalog apart from the product manifest. Candidates install only through Model Lab into `<app data>/model-lab/candidates`, are never listed by `list_models` or accepted by `select_model`, and are labelled `evaluationOnly` in every result. On 2026-10-10 each pin's commit, file size and LFS SHA-256 were re-checked against the Hugging Face revision API and each download URL answered a HEAD request (no weights downloaded); the license tags read Apache-2.0 for both Qwen entries and MIT for SEA-LION. The Gemma 4 E2B, Ministral 3 3B and LFM2.5-1.2B pins were added and checked the same way later on 2026-10-10 (Apache-2.0, Apache-2.0 and `lfm1.0`).

## Shortlist

| Candidate                           | Role                                                                   | Pinned GGUF (repo @ commit)                                                                                       | File                                       | Bytes         | LFS SHA-256                                                        |
| ----------------------------------- | ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------ | ------------- | ------------------------------------------------------------------ |
| Qwen3.5-0.8B (Q4_K_M)               | generation, small; alternative to Qwen3-0.6B                           | `unsloth/Qwen3.5-0.8B-GGUF` @ `6ab461498e2023f6e3c1baea90a8f0fe38ab64d0`                                          | `Qwen3.5-0.8B-Q4_K_M.gguf`                 | 532,517,120   | `bd258782e35f7f458f8aced1adc053e6e92e89bc735ba3be89d38a06121dc517` |
| Qwen3.5-2B (Q4_K_M)                 | generation, next size up in the same family; alternative to Qwen3-1.7B | `unsloth/Qwen3.5-2B-GGUF` @ `f6d5376be1edb4d416d56da11e5397a961aca8ae`                                            | `Qwen3.5-2B-Q4_K_M.gguf`                   | 1,280,835,840 | `aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223` |
| Gemma-SEA-LION-v4.5-E2B-IT (Q4_K_M) | generation, different family with Filipino post-training               | `aisingapore/Gemma-SEA-LION-v4.5-E2B-IT-GGUF` @ `3c0d3590d93771f3f3e8a879d812500651576171` (publisher's own GGUF) | `Gemma-SEA-LION-v4.5-E2B-IT-Q4_K_M.gguf`   | 3,427,879,360 | `624a18a8cfc3d8ee29752200f73dc02f5007eaac408d94309e8d8da27b2a7ed9` |
| Gemma 4 E2B (Q4_K_M)                | generation, the SEA-LION base without Filipino post-training           | `unsloth/gemma-4-E2B-it-GGUF` @ `0314792d7f1f7e229411f620751375812bb9faf2`                                        | `gemma-4-E2B-it-Q4_K_M.gguf`               | 3,106,738,272 | `740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8` |
| Ministral 3 3B Instruct (Q4_K_M)    | generation, different family                                           | `mistralai/Ministral-3-3B-Instruct-2512-GGUF` @ `eb599d408350ea2bb60452cb86be7c7b2fc28227` (publisher's own GGUF) | `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf` | 2,147,023,008 | `9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8` |
| LFM2.5-1.2B Instruct (Q4_K_M)       | generation, small and fast, different family                           | `LiquidAI/LFM2.5-1.2B-Instruct-GGUF` @ `8ed288026e23958ad9dfa92d53ed773a8eee7125` (publisher's own GGUF)          | `LFM2.5-1.2B-Instruct-Q4_K_M.gguf`         | 730,895,168   | `b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5` |

Download URL form: `https://huggingface.co/<repo>/resolve/<commit>/<file>`.

- Only the text GGUF is listed. The multimodal projector files (`mmproj-*.gguf`) are not needed for Folio's text tasks and are excluded.
- The runtime is the existing pinned llama.cpp **b11524**. Its `src/llama-arch.cpp` at that tag declares the `qwen35`, `gemma4`, `mistral3` and `lfm2` architectures (fetched from `ggml-org/llama.cpp` at ref `b11524`). Real loading is still unverified.

### Qwen3.5-0.8B and Qwen3.5-2B

- **Source model:** `Qwen/Qwen3.5-0.8B` @ `2fc06364715b967f1860aea9cf38778875588b17` and `Qwen/Qwen3.5-2B` @ `15852e8c16360a2fea060d615a32b45270f8a8fc`. Model type `qwen3_5` (`Qwen3_5ForConditionalGeneration`).
- **License:** Apache-2.0 (card front matter and repo tag); not gated.
- **Language coverage, claimed:** "Expanded support to 201 languages and dialects" (model card). The card does **not** list Tagalog or Filipino by name and reports **no Tagalog-specific evaluation**: its multilingual results are averaged over sets such as MMLU-ProX (29 languages) and WMT24++ (55 languages). Tagalog coverage is therefore **claimed in aggregate, not evaluated by the publisher**.
- **Chat and thinking:** the cards state both models run in **non-thinking mode by default**. Folio already sends `chat_template_kwargs: {enable_thinking: false}`.
- **GGUF provenance:** no first-party Qwen GGUF exists for these sizes (`Qwen/Qwen3.5-0.8B-GGUF` and `Qwen/Qwen3.5-2B-GGUF` are not found).
  - The pinned files are **third-party conversions by `unsloth`**, the same publisher as Folio's current Qwen3 Q4_K_M pins, chosen for a like-for-like Q4_K_M comparison. Trust rests on the pinned commit plus SHA-256, not on the host.
  - Cross-check option: `ggml-org/Qwen3.5-0.8B-GGUF` @ `8fea620810c4afa23dd6443f999a48574c1611a3` (`Qwen3.5-0.8B-Q4_0.gguf`, 563,036,064 B, `57d1997790d1744fba5b40a7317df71ea5e2acee28c47e78f0cce39c0703f8cf`), from the llama.cpp maintainers' organisation. ggml-org publishes no 2B GGUF.

### Gemma-SEA-LION-v4.5-E2B-IT

- **Source model:** `aisingapore/Gemma-SEA-LION-v4.5-E2B-IT` @ `c028f609f6f84dd0e9b669dad9dc1f9fd61b8bed`. It is post-trained from `google/gemma-4-E2B-it`. The card states **2.3B effective parameters (5.1B including embeddings)**, a 128K context and model type `gemma4`.
- **License:** the card front matter and tag say **MIT**, but its `license_link` points to the Gemma 4 license page, which Google's `gemma-4-E2B-it` card labels Apache 2.0. Both are permissive. **The mismatch should be confirmed with the publisher before any redistribution.** Folio itself downloads from the pinned URL and doesn't redistribute. Not gated.
- **Language coverage:** the card lists `fil` among its languages and says "fine-tuned on Burmese, Indonesian, **Filipino**, Malay, Tamil, Thai, and Vietnamese".
- **Evaluated by the publisher:** the card describes SEA-HELM evaluation including **MCQ-QA (TL)** and **Kalahi**, plus SEA-IFEval and SEA-MTBench. Results are published as an image and on `leaderboard.sea-lion.ai`. This is **publisher-reported evaluation**, not independent and not of Folio's tasks.
- **Card caveat:** "The model has not been aligned for safety."
- **Footprint:** the Q4_K_M file alone is about 3.43 GB, the largest candidate. RAM on the 8 GB / no-dedicated-GPU target is **unmeasured**.
- **Chat and thinking:** Gemma 4 uses standard system/user/assistant roles. Thinking is enabled only by a `<|think|>` token in the system prompt, and for E2B/E4B is off when that token is absent (Google's card). Folio doesn't add that token.

### Gemma 4 E2B

- **Source model:** `google/gemma-4-E2B-it` @ `3e22461f65e89153144f8adb70e3b8c2cc9845a7`, the base of SEA-LION v4.5 E2B. Measured beside SEA-LION to show what the Filipino post-training changes.
- **License:** Apache-2.0; not gated.
- **Language coverage, claimed:** "out-of-the-box support for 35+ languages, pre-trained on 140+", with no Tagalog-specific evaluation.
- **GGUF provenance:** a third-party `unsloth` Q4_K_M, for a like-for-like comparison with the other pins. Google's own `google/gemma-4-E2B-it-qat-q4_0-gguf` @ `675cff42a74c774d6cb76f76d8eacb49b48c9b93` (`gemma-4-E2B_q4_0-it.gguf`, 3,349,516,256 B, `fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634`) is a QAT Q4_0 and stays a cross-check option.
- **Footprint:** about 3.1 GB; RAM on the 8 GB target is unmeasured.

### Ministral 3 3B Instruct

- **Source model:** `mistralai/Ministral-3-3B-Instruct-2512` @ `b35d4dfe56c142746f54dbd64f579faab2744308`. llama.cpp architecture `mistral3`.
- **License:** Apache-2.0; not gated.
- **Language coverage:** the card lists en, fr, es, de, it, pt, nl, zh, ja, ko and ar. **Tagalog is not listed**, so Filipino and Taglish quality is expected to be the main risk.
- **Structured output:** its JSON strength matters less for Folio, which already constrains every response with a strict `json_schema`.
- **GGUF provenance:** the publisher's own Q4_K_M. Only the text GGUF is pinned.

### LFM2.5-1.2B Instruct

- **Source model:** `LiquidAI/LFM2.5-1.2B-Instruct` @ `0f604ada3f766f9f257460c4c9f0b5d6f69d431b`. llama.cpp architecture `lfm2`.
- **License:** **LFM Open License v1.0** (`lfm1.0`), the publisher's own and not OSI-approved. Its terms must be reviewed before any product use; the catalog carries this as a `licenseNote`.
- **Language coverage:** the card lists en, ar, zh, fr, de, ja, ko and es. **Tagalog is not listed.**
- **Footprint:** about 0.73 GB, the only candidate besides Qwen3.5-0.8B that fits beside the default install's size budget.
- **GGUF provenance:** the publisher's own Q4_K_M.

## Considered, not shortlisted

- **`sail/Sailor2-1B-Chat`** (Apache-2.0, Qwen2.5-based, card lists Tagalog). Only community GGUFs exist, and it is an older base. Not shortlisted.
- **`meta-llama/Llama-3.2-1B-Instruct`:** gated. Its official language list (`en, de, fr, it, pt, hi, es, th`) excludes Tagalog.
- **`google/gemma-3-1b-it`:** gated under the Gemma terms and superseded by Gemma 4.
- **`Qwen/Qwen3.5-4B`:** a possible later "stronger same-family" step. Deferred to keep the shortlist small and within the 8 GB target.
- **Embedding:** keep **multilingual-E5-small int8**. The Linux diagnostics found no retrieval weakness after the #4 fixes. `aisingapore/SEA-LION-E5-Embedding-600M` (MIT, @ `6a97cedd22714f3d4eb7c6fd1a452bf310899683`) is a possible future evaluation-only embedding candidate if Filipino retrieval weaknesses are observed. Not pinned now.

## What would make a candidate "supported"

- It measurably meets the required correctness outcomes on Folio's English, Filipino and Taglish tasks; summary correctness needs a human review.
- Then come acceptable efficiency (speed, process memory, retry needs) on Windows and macOS runners. These measurements describe the runner, not the 8 GB device.
- Finally, the user confirms the promotion.
- If none qualifies, report that no suitable candidate has been established.
