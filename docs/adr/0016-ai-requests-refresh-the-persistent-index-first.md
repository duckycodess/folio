# AI requests refresh the persistent index first, then read only it

Search, answers and request interpretation used an in-memory snapshot (#4's interim path): on any change to the folder they re-read every file and re-embedded every chunk, and they never read the persistent index (#3) or the embeddings stored for it (#27). That cost grows with the folder and holds all chunks and vectors in RAM, which does not fit an 8 GB machine.

**Each AI request brings the persistent index up to date, then queries it.** The refresh is an incremental `scan_workspace` (a stat per file; only changed files are read) followed by `sync_embeddings` for the chunks that have no vector in the current space, in small cancellable batches. The alternatives were to read only what is already stored (answers can be stale until the user presses a button) or to fill vectors in the background (a request can start before its vectors exist). Refreshing per request keeps answers current, costs about what the snapshot's folder walk cost, and shows progress on the first run.

**Persistent chunks are embedded with their title and path words.** The evidence gate was tuned on title-and-path passages, and a query naming a document should find it across languages. Embedding the chunk text alone would have moved the gate onto an untuned input. The stored-space input version is now `title-path-chunk-v2`, so vectors embedded from text alone stay in their own space and are never compared with these. The gate thresholds are still development defaults and are not measured for this space (chunks are up to 1,200 characters here, not 800); that measurement belongs to Model Lab (#77).

**Stale or changed text never reaches a prompt.** Queries read only documents with status `indexed`. Passages are bound to the revision the index holds, and before a prompt is built the documents behind its passages are hashed again; a passage of an older revision is dropped and its document is read again for the next request. This closes the one gap a scan cannot see: a file edited with the same size and modification time.

**Interpretation reads the files a target could mean, not the folder.** The model still sees only the request. After it answers, Folio reads the current text and chunks of at most eight documents whose names or text match the target description, and resolves the target from those and the metadata of every indexed document.

**Search and answers are busy during a Model Lab run.** Model Lab owns the embedding model while it measures, so these requests report `providerBusy` rather than silently answering with keyword search. Interpretation needs no embedding model and keeps working.

**Known costs, recorded rather than hidden.**

- Brute-force cosine over the stored vectors reads every vector of the current space for each query (about 1.5 KB per chunk at 384 dimensions). It is bounded in memory (scores, not text) but its time on a large folder is unmeasured; a cache is the next step if it matters.
- Keyword scoring takes its statistics from SQL and estimates average length in tokens from the candidates' own ratio, an approximation the keyword floor has not been measured against.
- A document whose title or path changes, with its chunk text unchanged, keeps vectors embedded with the old title or path words until its text changes.
- Vectors of spaces no longer in use stay in the database; pruning them is a follow-up.

Decided on 2026-10-10 for issue #92.
