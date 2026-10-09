//! AI context from the persistent index.
//!
//! Search, answers and request interpretation read chunks and vectors that the
//! index already holds instead of re-reading and re-embedding the folder. Each
//! request first brings the index up to date, cheaply: an incremental scan that
//! re-reads only files whose size or modification time changed, then an
//! embedding fill for only the chunks that have no vector in the current
//! embedding space, in small cancellable batches. Then it queries.
//!
//! Documents are evidence, not instructions. Only documents with status
//! `indexed` are read, and the documents behind the passages that go into a
//! prompt are hashed again first, so a stale or deleted chunk never reaches a
//! prompt.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::Duration;

use folio_core::chunking::Chunk;
use folio_core::contracts::{
    DocumentRecord, EmbeddingSpace as ProviderEmbeddingSpace, Language, SearchMethod,
    SearchResult, SourcePassage,
};
use folio_core::embeddings::QueryEmbedding;
use folio_core::generation::MAX_PASSAGES;
use folio_core::grounding;
use folio_core::retrieval::{
    self, Bm25Scorer, Bm25Stats, EvidenceGate, MIN_KEYWORD_SCORE, MIN_SEMANTIC_SCORE,
};
use rusqlite::Connection;
use serde::Serialize;

use crate::db::NativeResult;
use crate::embedding_sync::{
    self, ChunkStore, IndexChunkStore, PassageEmbedder, SyncLimits,
};
use crate::error::{error, ErrorCode};
use crate::index::{self, ChunkVector, PendingChunk, ScanOptions, StoredChunk};
use crate::workspace::{self, ScopedRoot};

/// Chunks read per leg before scoring. Bounds the text read for one request.
const KEYWORD_CANDIDATES: usize = 400;
const SEMANTIC_CANDIDATES: usize = 400;
/// Passages one document contributes to a result, as in the interim retriever.
const PASSAGES_PER_DOCUMENT: usize = 3;
/// Documents an interpretation may read in full to resolve one target.
const INTERPRETATION_CANDIDATES: usize = 8;
const LOCK_POLL: Duration = Duration::from_millis(50);

/// The embedding model behind a request: the one provider of passage and query
/// vectors. `provider_space` is `None` when no verified model is selected, and
/// the request then falls back to keyword search.
pub(crate) trait Embedder: PassageEmbedder {
    fn provider_space(&mut self) -> NativeResult<Option<ProviderEmbeddingSpace>>;
    fn embed_query(&mut self, text: &str, cancel: &AtomicBool) -> NativeResult<QueryEmbedding>;
}

/// Where a request looks.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Scope<'a> {
    Folder,
    /// One document, so another document cannot open the evidence gate for it.
    Document(&'a str),
}

impl<'a> Scope<'a> {
    fn document_id(self) -> Option<&'a str> {
        match self {
            Scope::Folder => None,
            Scope::Document(id) => Some(id),
        }
    }
}

/// Emitted while a request brings the index up to date, so a long first run
/// shows what it is doing instead of silently blocking.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparingProgress {
    pub workspace_id: String,
    /// `reading` files or `embedding` chunks.
    pub phase: &'static str,
    pub processed: usize,
    pub total: usize,
}

/// What `interpret_request` needs to resolve one target: every indexed
/// document's metadata, and the full text and chunks of only the few documents
/// the target description could mean.
pub(crate) struct InterpretationCorpus {
    pub documents: Vec<DocumentRecord>,
    pub contents: HashMap<String, String>,
    pub chunks: Vec<Chunk>,
}

/// A document the index holds but cannot search yet.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SkippedDocument {
    pub relative_path: String,
    pub reason: String,
}

/// What the persistent index holds for a folder, and how much of it a semantic
/// search can use. `method` is `hybrid` only when every chunk has a vector in the space of
/// the loaded embedding model; otherwise search is keyword search.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexStatus {
    pub workspace_id: Option<String>,
    pub document_count: usize,
    pub chunk_count: usize,
    /// Chunks with a vector in the current space; unknown until a request has
    /// loaded the embedding model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded_chunk_count: Option<usize>,
    pub method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_fingerprint: Option<String>,
    pub skipped_documents: Vec<SkippedDocument>,
}

impl IndexStatus {
    pub(crate) fn empty() -> Self {
        Self {
            workspace_id: None,
            document_count: 0,
            chunk_count: 0,
            embedded_chunk_count: None,
            method: "keyword",
            space_fingerprint: None,
            skipped_documents: Vec::new(),
        }
    }
}

/// Read-only: counts from the persistent index. `space_fingerprint` is the
/// stored space of the embedding model, when one is loaded.
pub(crate) fn status(
    conn: &Connection,
    workspace_id: &str,
    space_fingerprint: Option<&str>,
) -> NativeResult<IndexStatus> {
    let documents = index::list_documents(conn, workspace_id)?;
    let indexed = documents.iter().filter(|document| document.status == "indexed");
    let (embedded, chunks) = index::embedding_coverage(conn, workspace_id, space_fingerprint.unwrap_or(""))?;
    // Only a fully embedded folder is searched by meaning everywhere; a cancelled
    // fill leaves part of it keyword-only.
    let hybrid = space_fingerprint.is_some() && chunks > 0 && embedded == chunks;
    Ok(IndexStatus {
        workspace_id: Some(workspace_id.to_owned()),
        document_count: indexed.count(),
        chunk_count: chunks,
        embedded_chunk_count: space_fingerprint.map(|_| embedded),
        method: if hybrid { "hybrid" } else { "keyword" },
        space_fingerprint: space_fingerprint.map(str::to_owned),
        skipped_documents: documents
            .iter()
            .filter(|document| document.status != "indexed")
            .map(|document| SkippedDocument {
                relative_path: document.relative_path.clone(),
                reason: document
                    .status_message
                    .clone()
                    .unwrap_or_else(|| document.status.clone()),
            })
            .collect(),
    })
}

/// Whether a passage needs real evidence to be a candidate.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    Require,
    /// The user chose the file; its passages are candidates whatever they score.
    Bypass,
}

struct ScoredPassages {
    passages: Vec<(SourcePassage, f32)>,
    method: SearchMethod,
    space_fingerprint: Option<String>,
    /// Whether any passage in scope passed the evidence gate and semantic
    /// floor or the keyword floor, i.e. would be a candidate under
    /// `Gate::Require`, whatever the policy.
    matched: bool,
}

/// The passages an answer prompt may use.
pub(crate) struct PromptEvidence {
    pub(crate) passages: Vec<SourcePassage>,
    /// False only for a chosen file in which nothing passed the evidence gate
    /// or the keyword floor: its passages are then its closest or opening
    /// ones, sent because the user chose the file, not because they matched.
    pub(crate) matched: bool,
}

#[derive(Clone)]
struct SpaceInUse {
    fingerprint: String,
}

pub(crate) struct LocalIndex<'a, E: Embedder> {
    conn: &'a mut Connection,
    root: &'a ScopedRoot,
    scanning: &'a Mutex<()>,
    embedding_sync: &'a Mutex<()>,
    embedder: &'a mut E,
    cancel: &'a AtomicBool,
    progress: &'a mut dyn FnMut(PreparingProgress),
    scanned: bool,
    space: Option<Option<SpaceInUse>>,
}

fn cancelled() -> crate::error::FolioError {
    error(ErrorCode::Cancelled, "Preparing the folder was cancelled.")
}

/// Waits for a lock another request or Local Sync holds, giving up when the
/// request is cancelled.
fn lock_unless_cancelled<'m>(
    lock: &'m Mutex<()>,
    cancel: &AtomicBool,
) -> NativeResult<MutexGuard<'m, ()>> {
    loop {
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            // The guarded value is `()`, so a panic elsewhere left nothing broken.
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if cancel.load(Ordering::Acquire) {
                    return Err(cancelled());
                }
                std::thread::sleep(LOCK_POLL);
            }
        }
    }
}

/// The index's passage in the shape retrieval and generation share.
fn core_passage(passage: crate::contracts::SourcePassage) -> SourcePassage {
    SourcePassage {
        document_id: passage.document_id,
        document_content_hash: passage.document_content_hash,
        offset_unit: folio_core::contracts::OffsetUnit::Utf8Byte,
        start: passage.start,
        end: passage.end,
        text: passage.text,
        page: passage.page,
    }
}

fn record_of(document: &index::IndexedDocument) -> DocumentRecord {
    DocumentRecord {
        id: document.id.clone(),
        workspace_id: document.workspace_id.clone(),
        relative_path: document.relative_path.clone(),
        name: document.name.clone(),
        title: document.title.clone(),
        language: Language::Unknown,
        media_type: document.media_type.clone(),
        size_bytes: document.size_bytes,
        modified_at_ms: document.modified_at_ms,
        content: None,
        content_hash: Some(document.content_hash.clone()),
    }
}

/// Reports each stored batch while the fill runs.
struct ProgressStore<'a, S: ChunkStore> {
    inner: S,
    workspace_id: String,
    already_embedded: usize,
    total: usize,
    stored: usize,
    progress: &'a mut dyn FnMut(PreparingProgress),
}

impl<S: ChunkStore> ChunkStore for ProgressStore<'_, S> {
    fn pending(&mut self, limit: usize) -> NativeResult<Vec<PendingChunk>> {
        self.inner.pending(limit)
    }

    fn put(&mut self, items: &[ChunkVector]) -> NativeResult<usize> {
        let stored = self.inner.put(items)?;
        self.stored += stored;
        (self.progress)(PreparingProgress {
            workspace_id: self.workspace_id.clone(),
            phase: "embedding",
            processed: (self.already_embedded + self.stored).min(self.total),
            total: self.total,
        });
        Ok(stored)
    }
}

impl<'a, E: Embedder> LocalIndex<'a, E> {
    pub(crate) fn new(
        conn: &'a mut Connection,
        root: &'a ScopedRoot,
        scanning: &'a Mutex<()>,
        embedding_sync: &'a Mutex<()>,
        embedder: &'a mut E,
        cancel: &'a AtomicBool,
        progress: &'a mut dyn FnMut(PreparingProgress),
    ) -> Self {
        Self {
            conn,
            root,
            scanning,
            embedding_sync,
            embedder,
            cancel,
            progress,
            scanned: false,
            space: None,
        }
    }

    /// Stops a request whose user cancelled it, for the step between preparing
    /// the index and holding the generation slot.
    pub(crate) fn ensure_not_cancelled(&self) -> NativeResult<()> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        Ok(())
    }

    /// Brings the index in line with the files. Run once per request: it stats
    /// every file but reads only those whose size or modification time changed.
    pub(crate) fn refresh_files(&mut self) -> NativeResult<()> {
        if self.scanned {
            return Ok(());
        }
        let _scanning = lock_unless_cancelled(self.scanning, self.cancel)?;
        let workspace_id = self.root.id.clone();
        let progress = &mut *self.progress;
        let summary = index::scan_workspace(
            self.conn,
            self.root,
            &ScanOptions::now(),
            self.cancel,
            &mut |reading| {
                progress(PreparingProgress {
                    workspace_id: workspace_id.clone(),
                    phase: "reading",
                    processed: reading.processed,
                    total: reading.total,
                })
            },
        )?;
        if summary.cancelled {
            return Err(cancelled());
        }
        self.scanned = true;
        Ok(())
    }

    /// Files, then vectors: after this every indexed chunk has a vector in the
    /// current embedding space, or there is no model and this is `None`.
    fn ensure_embedded(&mut self) -> NativeResult<Option<SpaceInUse>> {
        if let Some(space) = &self.space {
            return Ok(space.clone());
        }
        self.refresh_files()?;
        let space = match self.embedder.provider_space()? {
            None => None,
            Some(provider) => Some(self.fill_vectors(provider)?),
        };
        self.space = Some(space.clone());
        Ok(space)
    }

    fn fill_vectors(&mut self, provider: ProviderEmbeddingSpace) -> NativeResult<SpaceInUse> {
        let stored = embedding_sync::stored_index_space(&provider)?;
        let fingerprint = index::register_space(self.conn, &stored)?;
        let workspace_id = self.root.id.clone();
        let (embedded, total) = index::embedding_coverage(self.conn, &workspace_id, &fingerprint)?;
        // With every chunk embedded there is nothing to serialize, so a
        // request never queues behind a Graph refresh's or another request's
        // fill just to find that out.
        if embedded >= total {
            return Ok(SpaceInUse { fingerprint });
        }
        let _syncing = lock_unless_cancelled(self.embedding_sync, self.cancel)?;
        // Whoever held the lock may have filled some or all of it meanwhile.
        let (embedded, total) = index::embedding_coverage(self.conn, &workspace_id, &fingerprint)?;
        if embedded < total {
            (self.progress)(PreparingProgress {
                workspace_id: workspace_id.clone(),
                phase: "embedding",
                processed: embedded,
                total,
            });
            let mut store = ProgressStore {
                inner: IndexChunkStore::new(self.conn, workspace_id.clone(), fingerprint.clone()),
                workspace_id: workspace_id.clone(),
                already_embedded: embedded,
                total,
                stored: 0,
                progress: &mut *self.progress,
            };
            let summary = embedding_sync::sync_embeddings(
                &mut store,
                self.embedder,
                &provider,
                &fingerprint,
                workspace_id,
                self.cancel,
                SyncLimits::default(),
            )?;
            if summary.cancelled {
                return Err(cancelled());
            }
        }
        Ok(SpaceInUse { fingerprint })
    }

    /// "Prepare now": files, then vectors, then what the index holds.
    pub(crate) fn prepare(&mut self) -> NativeResult<IndexStatus> {
        let space = self.ensure_embedded()?;
        status(
            self.conn,
            &self.root.id,
            space.as_ref().map(|space| space.fingerprint.as_str()),
        )
    }

    /// Ranked documents with their passages. Hybrid (cosine over the stored
    /// vectors, FTS5 keywords as a bounded tiebreak) when an embedding model is
    /// available, otherwise FTS5 keyword search labelled `keyword`.
    pub(crate) fn search(
        &mut self,
        query: &str,
        scope: Scope<'_>,
        limit: usize,
    ) -> NativeResult<Vec<SearchResult>> {
        let space = self.ensure_embedded()?;
        self.retrieve(query, scope.document_id(), limit, space.as_ref())
    }

    /// The passages that may go into an answer prompt, within
    /// `MAX_PASSAGES` and the evidence byte budget, all from documents that are
    /// still what the index holds. For `Scope::Document` the user has chosen
    /// the file, so the evidence gate does not apply: its best-ranked passages
    /// are sent, or its opening passages when nothing ranks.
    pub(crate) fn prompt_evidence(
        &mut self,
        question: &str,
        scope: Scope<'_>,
    ) -> NativeResult<PromptEvidence> {
        let mut space = self.ensure_embedded()?;
        let mut retried = false;
        loop {
            let (passages, matched) = self.passages_for(question, scope, space.as_ref())?;
            let (current, dropped) = self.current_passages_only(passages)?;
            // A document edited behind the scan's back was just read again, so
            // look once more: its fresh chunks may answer.
            if dropped && !retried {
                retried = true;
                self.space = None;
                space = self.ensure_embedded()?;
                continue;
            }
            return Ok(PromptEvidence {
                passages: current,
                matched,
            });
        }
    }

    fn passages_for(
        &mut self,
        question: &str,
        scope: Scope<'_>,
        space: Option<&SpaceInUse>,
    ) -> NativeResult<(Vec<SourcePassage>, bool)> {
        Ok(match scope {
            // Folder passages all passed the gate; none at all is
            // insufficient evidence, not an unmatched answer.
            Scope::Folder => {
                let results =
                    self.retrieve(question, None, MAX_PASSAGES, space)?;
                let passages = grounding::fit_evidence_budget(
                    results
                        .into_iter()
                        .flat_map(|result| result.passages)
                        .take(MAX_PASSAGES)
                        .collect(),
                );
                (passages, true)
            }
            Scope::Document(id) => {
                let scored = self.scored_passages(question, Some(id), space, Gate::Bypass)?;
                let mut ranked = scored.passages;
                ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
                let opening = index::leading_chunks(self.conn, &self.root.id, id, MAX_PASSAGES)?
                    .into_iter()
                    .map(|chunk| core_passage(chunk.passage))
                    .collect();
                let passages = grounding::chosen_document_passages(
                    ranked.into_iter().map(|(passage, _)| passage).collect(),
                    opening,
                );
                (passages, scored.matched)
            }
        })
    }

    fn retrieve(
        &mut self,
        query: &str,
        document_id: Option<&str>,
        limit: usize,
        space: Option<&SpaceInUse>,
    ) -> NativeResult<Vec<SearchResult>> {
        let scored = self.scored_passages(query, document_id, space, Gate::Require)?;
        self.group(
            scored.passages,
            scored.method,
            scored.space_fingerprint,
            limit,
        )
    }

    /// Candidate passages with their scores. Under `Gate::Require` a passage
    /// needs real evidence (the evidence gate and the semantic or keyword
    /// floor); under `Gate::Bypass` every scored passage is a candidate.
    fn scored_passages(
        &mut self,
        query: &str,
        document_id: Option<&str>,
        space: Option<&SpaceInUse>,
        gate_policy: Gate,
    ) -> NativeResult<ScoredPassages> {
        let terms = retrieval::query_terms(query);
        let keyword = self.keyword_scores(&terms, document_id)?;
        let required = gate_policy == Gate::Require;
        let Some(space) = space else {
            let matched = keyword.values().any(|(_, score)| *score >= MIN_KEYWORD_SCORE);
            let passages = keyword
                .into_values()
                .filter(|(_, score)| !required || *score >= MIN_KEYWORD_SCORE)
                .map(|(chunk, score)| (core_passage(chunk.passage), score))
                .collect();
            return Ok(ScoredPassages {
                passages,
                method: SearchMethod::Keyword,
                space_fingerprint: None,
                matched,
            });
        };

        let embedding = self.embedder.embed_query(query, self.cancel)?;
        let query_space = embedding_sync::stored_space_fingerprint(&embedding.space)?;
        if query_space != space.fingerprint {
            return Err(error(
                ErrorCode::EmbeddingSpaceMismatch,
                "The query was embedded in a different space than the stored vectors.",
            )
            .with_detail("expectedSpace", space.fingerprint.clone())
            .with_detail("actualSpace", query_space));
        }
        let cosines = index::vector_scores(
            self.conn,
            &self.root.id,
            &space.fingerprint,
            &embedding.vector,
            document_id,
        )?;
        // The gate looks at every cosine in scope, not only the candidates.
        let gate = EvidenceGate::from_cosines(cosines.iter().map(|score| score.cosine).collect());
        let mut ranked = cosines
            .iter()
            .filter(|score| !required || (gate.passed && score.cosine >= MIN_SEMANTIC_SCORE))
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| b.cosine.total_cmp(&a.cosine));
        ranked.truncate(SEMANTIC_CANDIDATES);
        let cosine_of = cosines
            .iter()
            .map(|score| (score.chunk_id, score.cosine))
            .collect::<HashMap<_, _>>();

        let mut chunks: HashMap<i64, StoredChunk> = HashMap::new();
        let semantic_ids = ranked.iter().map(|score| score.chunk_id).collect::<Vec<_>>();
        let mut keyword_of: HashMap<i64, f32> = HashMap::new();
        for (chunk_id, (chunk, score)) in keyword {
            keyword_of.insert(chunk_id, score);
            chunks.insert(chunk_id, chunk);
        }
        let missing = semantic_ids
            .into_iter()
            .filter(|id| !chunks.contains_key(id))
            .collect::<Vec<_>>();
        for chunk in index::stored_chunks(self.conn, &self.root.id, &missing)? {
            chunks.insert(chunk.chunk_id, chunk);
        }
        let scores_of = |chunk_id: i64| {
            let cosine = cosine_of.get(&chunk_id).copied().unwrap_or(0.0);
            let keyword = keyword_of.get(&chunk_id).copied().unwrap_or(0.0);
            (cosine, keyword, retrieval::admits(&gate, cosine, keyword))
        };
        let matched = chunks.keys().any(|chunk_id| scores_of(*chunk_id).2);
        let passages = chunks
            .into_values()
            .filter_map(|chunk| {
                let (cosine, keyword, admitted) = scores_of(chunk.chunk_id);
                (!required || admitted)
                    .then(|| (core_passage(chunk.passage), retrieval::fused(cosine, keyword)))
            })
            .collect();
        Ok(ScoredPassages {
            passages,
            method: SearchMethod::Hybrid,
            space_fingerprint: Some(space.fingerprint.clone()),
            matched,
        })
    }

    /// Normalized BM25 for the FTS5 candidates, keyed by chunk. Statistics come
    /// from SQL, so no chunk text beyond the candidates is read. Average length
    /// in tokens is estimated from the candidates' own token-per-character
    /// ratio, which is an approximation the thresholds have not been measured on.
    fn keyword_scores(
        &mut self,
        terms: &[String],
        document_id: Option<&str>,
    ) -> NativeResult<HashMap<i64, (StoredChunk, f32)>> {
        if terms.is_empty() {
            return Ok(HashMap::new());
        }
        let hits = index::keyword_hits(
            self.conn,
            &self.root.id,
            terms,
            document_id,
            KEYWORD_CANDIDATES,
        )?;
        if hits.is_empty() {
            return Ok(HashMap::new());
        }
        let stats = index::keyword_stats(self.conn, &self.root.id, terms, document_id)?;
        let tokens = hits
            .iter()
            .map(|hit| retrieval::tokens(&hit.passage.text))
            .collect::<Vec<_>>();
        // Characters, like SQLite's `length()` behind `average_chars`, not bytes:
        // Filipino and Taglish text has multi-byte letters.
        let characters = hits
            .iter()
            .map(|hit| hit.passage.text.chars().count())
            .sum::<usize>();
        let token_count = tokens.iter().map(Vec::len).sum::<usize>();
        let tokens_per_character = if characters == 0 {
            0.0
        } else {
            token_count as f32 / characters as f32
        };
        let scorer = Bm25Scorer::new(
            terms,
            &Bm25Stats {
                chunk_count: stats.chunk_count,
                average_length: stats.average_chars * tokens_per_character,
                document_frequency: stats.document_frequency,
            },
        );
        Ok(hits
            .into_iter()
            .zip(tokens)
            .map(|(hit, tokens)| {
                let score = scorer.score(&tokens);
                (hit.chunk_id, (hit, score))
            })
            .collect())
    }

    fn group(
        &mut self,
        scored: Vec<(SourcePassage, f32)>,
        method: SearchMethod,
        space_fingerprint: Option<String>,
        limit: usize,
    ) -> NativeResult<Vec<SearchResult>> {
        let mut records: HashMap<String, DocumentRecord> = HashMap::new();
        for (passage, _) in &scored {
            if records.contains_key(&passage.document_id) {
                continue;
            }
            // A document removed since the read simply has no result.
            if let Ok(document) =
                index::get_document(self.conn, &self.root.id, &passage.document_id)
            {
                records.insert(passage.document_id.clone(), record_of(&document));
            }
        }
        let by_id = records
            .iter()
            .map(|(id, record)| (id.as_str(), record))
            .collect();
        Ok(retrieval::group_passages(
            by_id,
            scored,
            method,
            space_fingerprint,
            PASSAGES_PER_DOCUMENT,
            limit,
        ))
    }

    /// Drops passages whose document no longer matches the revision the index
    /// holds (a file edited in a way a scan cannot see, such as the same size
    /// and modification time), and re-reads those documents so the next request
    /// starts from the current text. Bounded: at most the documents behind the
    /// passages, each read under the index's own size limit.
    fn current_passages_only(
        &mut self,
        passages: Vec<SourcePassage>,
    ) -> NativeResult<(Vec<SourcePassage>, bool)> {
        let mut verdicts: HashMap<String, bool> = HashMap::new();
        for passage in &passages {
            if verdicts.contains_key(&passage.document_id) {
                continue;
            }
            let current = match index::get_document(self.conn, &self.root.id, &passage.document_id)
            {
                Ok(document) => {
                    workspace::bounded_document_hash(&self.root.path, &document.relative_path)
                        .is_ok_and(|hash| {
                            hash == document.content_hash
                                && hash == passage.document_content_hash
                        })
                }
                Err(_) => false,
            };
            verdicts.insert(passage.document_id.clone(), current);
        }
        let changed = verdicts
            .iter()
            .filter(|(_, current)| !**current)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if !changed.is_empty() {
            let _scanning = lock_unless_cancelled(self.scanning, self.cancel)?;
            // Best effort: a document that cannot be read now stays marked by
            // the recheck, and this request simply goes without it.
            let _ = index::recheck_documents(self.conn, self.root, &changed, index::now_ms());
        }
        let dropped = !changed.is_empty();
        Ok((
            passages
                .into_iter()
                .filter(|passage| verdicts.get(&passage.document_id).copied().unwrap_or(false))
                .collect(),
            dropped,
        ))
    }

    /// Metadata of every indexed document, with the current text and chunks of
    /// only the few documents `target_description` could name. Nothing else is
    /// read, however large the folder.
    pub(crate) fn interpretation_corpus(
        &mut self,
        target_description: Option<&str>,
        chosen_document_id: Option<&str>,
    ) -> NativeResult<InterpretationCorpus> {
        self.refresh_files()?;
        let mut documents = index::list_documents(self.conn, &self.root.id)?
            .into_iter()
            .filter(|document| document.status == "indexed")
            .map(|document| record_of(&document))
            .collect::<Vec<_>>();
        let terms = target_description
            .map(retrieval::query_terms)
            .unwrap_or_default();

        // Documents whose name or path says it first, then those whose text
        // mentions the words.
        let mut candidates = documents
            .iter()
            .map(|document| {
                let name = document.relative_path.to_lowercase();
                let words = retrieval::tokens(&name);
                let matched = terms.iter().filter(|term| words.contains(term)).count();
                (matched, document.id.clone())
            })
            .filter(|(matched, _)| *matched > 0)
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        // A file the user picked is read first, whatever the description says.
        let mut chosen = chosen_document_id
            .map(str::to_owned)
            .into_iter()
            .collect::<Vec<_>>();
        chosen.extend(
            candidates
                .into_iter()
                .map(|(_, id)| id)
                .filter(|id| Some(id.as_str()) != chosen_document_id),
        );
        let mut seen = chosen.iter().cloned().collect::<HashSet<_>>();
        for hit in index::keyword_hits(self.conn, &self.root.id, &terms, None, 50)? {
            if seen.insert(hit.passage.document_id.clone()) {
                chosen.push(hit.passage.document_id);
            }
        }
        chosen.truncate(INTERPRETATION_CANDIDATES);

        let mut contents = HashMap::new();
        let mut chunks = Vec::new();
        for id in chosen {
            let Some(record) = documents.iter_mut().find(|record| record.id == id) else {
                continue;
            };
            // The file now, not the index's copy of it. A file that cannot be
            // read has no content, so a proposal for it asks to try again.
            let Ok(text) = workspace::read_text(&self.root.path, &record.relative_path) else {
                continue;
            };
            let indexed_hash = record.content_hash.clone();
            record.content_hash = Some(text.content_hash.clone());
            if indexed_hash.as_deref() == Some(text.content_hash.as_str()) {
                chunks.extend(
                    index::document_chunks(self.conn, &self.root.id, &id)?
                        .into_iter()
                        .map(|stored| chunk_of(&stored, &text.content_hash)),
                );
            }
            contents.insert(id, text.content);
        }
        Ok(InterpretationCorpus {
            documents,
            contents,
            chunks,
        })
    }
}

/// An index chunk in the shape interpretation expects. Its `content_hash` is
/// the document revision, as for the interim chunker.
fn chunk_of(stored: &StoredChunk, document_hash: &str) -> Chunk {
    let passage = &stored.passage;
    Chunk {
        document_id: passage.document_id.clone(),
        ordinal: stored.ordinal,
        start: passage.start,
        end: passage.end,
        text: passage.text.clone(),
        content_hash: document_hash.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FolioError;
    use crate::index::tests::{fixture_workspace, id_of};
    use std::fs;

    /// A tiny deterministic multilingual "model": each concept is a set of
    /// English, Filipino and Taglish words, and a text embeds to the set of
    /// concepts it mentions. It exercises the storage, the spaces and the
    /// hybrid merge over the real index; it says nothing about the quality of
    /// the real multilingual-e5 model.
    const CONCEPTS: [&[&str]; 6] = [
        &["deadline", "huling", "pagpasa", "magpasa", "submission", "submit"],
        &["project", "proyekto", "plan", "plano", "learning"],
        &["budget", "money", "pera", "spending", "expense"],
        &["math", "matematika", "mathematics", "vectors", "derivatives", "probability"],
        &["travel", "terminal", "payong", "charger", "weather", "bumiyahe"],
        &["interview", "interviews", "panayam", "volunteer", "boluntaryong", "consent", "pahintulot"],
    ];

    fn concept_vector(text: &str) -> Vec<f32> {
        let words = retrieval::tokens(text);
        let mut vector = CONCEPTS
            .iter()
            .map(|concept| {
                if words.iter().any(|word| concept.contains(&word.as_str())) {
                    1.0
                } else {
                    0.0
                }
            })
            .collect::<Vec<f32>>();
        let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
        if norm > 0.0 {
            vector.iter_mut().for_each(|value| *value /= norm);
        }
        vector
    }

    fn concept_space(revision: &str) -> ProviderEmbeddingSpace {
        ProviderEmbeddingSpace {
            model_id: "concept-model".into(),
            revision: revision.into(),
            quantization: "test".into(),
            dimensions: CONCEPTS.len(),
            preprocessing_fingerprint: "concept-input-v1".into(),
        }
    }

    struct ConceptEmbedder {
        revision: String,
        installed: bool,
        /// What `embed_query` claims to have embedded in, when not the model's space.
        query_space: Option<ProviderEmbeddingSpace>,
        passages_embedded: usize,
        queries_embedded: usize,
        batches: usize,
        cancel_on_batch: Option<usize>,
    }

    impl ConceptEmbedder {
        fn new(revision: &str) -> Self {
            Self {
                revision: revision.into(),
                installed: true,
                query_space: None,
                passages_embedded: 0,
                queries_embedded: 0,
                batches: 0,
                cancel_on_batch: None,
            }
        }
    }

    impl PassageEmbedder for ConceptEmbedder {
        fn embed_batch(
            &mut self,
            texts: &[String],
            cancel: &AtomicBool,
        ) -> NativeResult<(ProviderEmbeddingSpace, Vec<Vec<f32>>)> {
            self.batches += 1;
            if self.cancel_on_batch == Some(self.batches) {
                cancel.store(true, Ordering::Release);
                return Err(error(ErrorCode::Cancelled, "Embedding cancelled."));
            }
            self.passages_embedded += texts.len();
            Ok((
                concept_space(&self.revision),
                texts.iter().map(|text| concept_vector(text)).collect(),
            ))
        }
    }

    impl Embedder for ConceptEmbedder {
        fn provider_space(&mut self) -> NativeResult<Option<ProviderEmbeddingSpace>> {
            Ok(self.installed.then(|| concept_space(&self.revision)))
        }

        fn embed_query(&mut self, text: &str, _cancel: &AtomicBool) -> NativeResult<QueryEmbedding> {
            self.queries_embedded += 1;
            Ok(QueryEmbedding {
                space: self
                    .query_space
                    .clone()
                    .unwrap_or_else(|| concept_space(&self.revision)),
                vector: concept_vector(text),
            })
        }
    }

    struct Harness {
        folder: tempfile::TempDir,
        conn: Connection,
        root: ScopedRoot,
        scanning: Mutex<()>,
        syncing: Mutex<()>,
        cancel: AtomicBool,
    }

    impl Harness {
        fn fixtures() -> Self {
            let (folder, conn, root) = fixture_workspace();
            Self::from(folder, conn, root)
        }

        fn from(folder: tempfile::TempDir, conn: Connection, root: ScopedRoot) -> Self {
            Self {
                folder,
                conn,
                root,
                scanning: Mutex::new(()),
                syncing: Mutex::new(()),
                cancel: AtomicBool::new(false),
            }
        }

        /// One AI request: a fresh `LocalIndex`, as each command builds.
        fn request<R>(
            &mut self,
            embedder: &mut ConceptEmbedder,
            work: impl FnOnce(&mut LocalIndex<'_, ConceptEmbedder>) -> NativeResult<R>,
        ) -> (NativeResult<R>, Vec<PreparingProgress>) {
            let mut events = Vec::new();
            let mut sink = |event: PreparingProgress| events.push(event);
            let mut index = LocalIndex::new(
                &mut self.conn,
                &self.root,
                &self.scanning,
                &self.syncing,
                embedder,
                &self.cancel,
                &mut sink,
            );
            let result = work(&mut index);
            drop(index);
            (result, events)
        }

        fn search(&mut self, embedder: &mut ConceptEmbedder, query: &str) -> Vec<SearchResult> {
            self.request(embedder, |index| index.search(query, Scope::Folder, 10))
                .0
                .unwrap()
        }

        fn chunk_total(&self) -> usize {
            self.conn
                .query_row(
                    "SELECT count(*) FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.status = 'indexed'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap() as usize
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.folder.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    fn paths(results: &[SearchResult]) -> Vec<&str> {
        results
            .iter()
            .map(|result| result.document.relative_path.as_str())
            .collect()
    }

    #[test]
    fn status_reports_what_is_indexed_and_how_much_of_it_has_vectors() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let before = status(&harness.conn, &harness.root.id, None).unwrap();
        assert_eq!(before.document_count, 0, "nothing has been indexed yet");

        let (prepared, _) = harness.request(&mut embedder, |index| index.prepare());
        let prepared = prepared.unwrap();
        let total = harness.chunk_total();
        assert_eq!(prepared.chunk_count, total);
        assert_eq!(prepared.embedded_chunk_count, Some(total));
        assert_eq!(prepared.method, "hybrid");
        assert!(prepared.space_fingerprint.is_some() && prepared.document_count > 10);

        let keyword = status(&harness.conn, &harness.root.id, None).unwrap();
        assert_eq!((keyword.method, keyword.embedded_chunk_count), ("keyword", None));
        let other_space =
            status(&harness.conn, &harness.root.id, Some("folio-space-v1/other/r9/x/2/y")).unwrap();
        assert_eq!((other_space.method, other_space.embedded_chunk_count), ("keyword", Some(0)));

        fs::write(harness.folder.path().join("notes/broken.md"), [0xff, 0xfe]).unwrap();
        let (again, _) = harness.request(&mut embedder, |index| index.prepare());
        let again = again.unwrap();
        assert!(again
            .skipped_documents
            .iter()
            .any(|skipped| skipped.relative_path == "notes/broken.md"));
    }

    #[test]
    fn a_repeat_request_on_an_unchanged_folder_embeds_nothing_new() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let (first, events) = harness.request(&mut embedder, |index| {
            index.search("project submission deadline", Scope::Folder, 10)
        });
        first.unwrap();
        let total = harness.chunk_total();
        assert!(total > 0);
        assert_eq!(embedder.passages_embedded, total, "the first request fills the index");
        assert!(
            events.iter().any(|event| event.phase == "reading")
                && events.iter().any(|event| event.phase == "embedding" && event.processed == total),
            "a first run reports what it is doing: {events:?}"
        );

        let (again, events) = harness.request(&mut embedder, |index| {
            index.search("project submission deadline", Scope::Folder, 10)
        });
        again.unwrap();
        assert_eq!(embedder.passages_embedded, total, "nothing was embedded again");
        assert_eq!(embedder.queries_embedded, 2);
        assert!(
            events.iter().all(|event| event.phase != "embedding"),
            "no fill ran: {events:?}"
        );
    }

    #[test]
    fn english_filipino_and_taglish_requests_find_the_project_across_languages() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        for query in [
            "project submission deadline",
            "huling araw ng pagpasa ng proyekto",
            "yung deadline ng project plan",
        ] {
            let results = harness.search(&mut embedder, query);
            let found = paths(&results);
            assert!(found.contains(&"projects/project-plan.md"), "{query}: {found:?}");
            assert!(found.contains(&"notes/tala-sa-proyekto.md"), "{query}: {found:?}");
            assert!(found.contains(&"meetings/meeting-notes.md"), "{query}: {found:?}");
            assert!(!found.contains(&"personal/travel-notes.md"), "{query}: {found:?}");
            assert!(results.iter().all(|result| result.method == SearchMethod::Hybrid));
            assert!(results.iter().all(|result| result.space_fingerprint.is_some()));
        }
        // A Filipino question about weather and an umbrella finds the Taglish travel note.
        let travel = harness.search(&mut embedder, "payong at weather bago bumiyahe");
        assert_eq!(paths(&travel), ["personal/travel-notes.md"]);
        // A request nothing in the folder is about has no evidence.
        assert!(harness.search(&mut embedder, "quantum chromodynamics").is_empty());
    }

    #[test]
    fn a_new_embedding_space_fills_separately_and_never_reads_the_old_vectors() {
        let mut harness = Harness::fixtures();
        let mut first = ConceptEmbedder::new("r1");
        let before = harness.search(&mut first, "project submission deadline");
        let total = harness.chunk_total();

        let mut second = ConceptEmbedder::new("r2");
        let after = harness.search(&mut second, "project submission deadline");
        assert_eq!(second.passages_embedded, total, "a model change rebuilds, it does not reuse r1's vectors");
        assert_eq!(paths(&before), paths(&after));

        let space_of = |revision: &str| {
            let provider = concept_space(revision);
            embedding_sync::stored_space_fingerprint(&provider).unwrap()
        };
        let (old, new) = (space_of("r1"), space_of("r2"));
        assert_ne!(old, new);
        for fingerprint in [&old, &new] {
            let (embedded, all) =
                index::embedding_coverage(&harness.conn, &harness.root.id, fingerprint).unwrap();
            assert_eq!((embedded, all), (total, total), "each space holds its own full set");
        }
        assert!(after
            .iter()
            .all(|result| result.space_fingerprint.as_deref() == Some(new.as_str())));
    }

    #[test]
    fn a_query_embedded_in_another_space_is_refused_not_compared() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        embedder.query_space = Some(concept_space("r2"));
        let (result, _) = harness.request(&mut embedder, |index| {
            index.search("project submission deadline", Scope::Folder, 10)
        });
        let failure = result.unwrap_err();
        assert_eq!(failure.code, ErrorCode::EmbeddingSpaceMismatch);
    }

    #[test]
    fn without_an_embedding_model_search_is_keyword_and_says_so() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        embedder.installed = false;
        let results = harness.search(&mut embedder, "deadline");
        let found = paths(&results);
        assert!(found.contains(&"projects/project-plan.md"), "{found:?}");
        assert!(results.iter().all(|result| result.method == SearchMethod::Keyword));
        assert!(results.iter().all(|result| result.space_fingerprint.is_none()));
        assert_eq!(embedder.passages_embedded + embedder.queries_embedded, 0);
    }

    #[test]
    fn a_changed_file_is_never_served_from_its_old_chunks() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        harness.search(&mut embedder, "project submission deadline");

        harness.write(
            "projects/project-plan.md",
            "# Household\n\nMoney, budget and spending for the month.\n",
        );
        let after = harness.search(&mut embedder, "project submission deadline");
        let found = paths(&after);
        assert!(!found.contains(&"projects/project-plan.md"), "{found:?}");
        assert!(found.contains(&"archive/project-plan-copy.md"), "its untouched copy still matches");

        let budget = harness.search(&mut embedder, "budget money");
        let plan = budget
            .iter()
            .find(|result| result.document.relative_path == "projects/project-plan.md")
            .expect("the edited file is found by what it says now");
        assert!(plan.passages.iter().all(|passage| passage.text.contains("Money")));
    }

    #[test]
    fn an_edit_a_scan_cannot_see_is_caught_by_the_revision_check_before_the_prompt() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let plan = id_of(&harness.root, "projects/project-plan.md");
        let (before, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("project submission deadline", Scope::Folder).map(|evidence| evidence.passages)
        });
        assert!(before.unwrap().iter().any(|passage| passage.document_id == plan));

        // Same size and modification time: the scan's stat check sees no change.
        let path = harness.folder.path().join("projects/project-plan.md");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let text = fs::read_to_string(&path).unwrap().replace("October 20", "October 21");
        fs::write(&path, text).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();

        let (during, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("project submission deadline", Scope::Folder).map(|evidence| evidence.passages)
        });
        let during = during.unwrap();
        // The stale passage is dropped, the file is read again, and the request
        // looks once more, so it is answered from the new revision rather than
        // going without that file.
        assert!(
            during
                .iter()
                .filter(|passage| passage.document_id == plan)
                .all(|passage| !passage.text.contains("October 20")),
            "a passage of the old revision never reaches a prompt"
        );
        let current = during
            .iter()
            .find(|passage| passage.document_id == plan)
            .expect("the re-read file answers in the same request");
        assert!(current.text.contains("October 21"));
        assert!(during.len() > 1, "other documents still answer");
    }

    #[test]
    fn unreadable_and_deleted_documents_leave_no_evidence() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        harness.search(&mut embedder, "project submission deadline");
        let paalala = "notes/tala-sa-proyekto.md";
        let meeting = "meetings/meeting-notes.md";
        fs::write(harness.folder.path().join(paalala), [0xff, 0xfe, 0xfd, 0x00]).unwrap();
        fs::remove_file(harness.folder.path().join(meeting)).unwrap();
        let after = harness.search(&mut embedder, "project submission deadline");
        let found = paths(&after);
        assert!(!found.contains(&paalala), "stale document: {found:?}");
        assert!(!found.contains(&meeting), "deleted document: {found:?}");
        assert!(found.contains(&"projects/project-plan.md"));
    }

    #[test]
    fn evidence_passages_match_the_current_file_so_citations_validate() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let (passages, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("huling araw ng pagpasa ng proyekto", Scope::Folder).map(|evidence| evidence.passages)
        });
        let passages = passages.unwrap();
        assert!(!passages.is_empty() && passages.len() <= MAX_PASSAGES);
        for passage in passages {
            let relative = passage.document_id.split_once(':').unwrap().1;
            let bytes = fs::read(harness.folder.path().join(relative)).unwrap();
            assert_eq!(passage.document_content_hash, crate::identity::content_hash(&bytes));
            let text = String::from_utf8(bytes).unwrap();
            assert_eq!(&text[passage.start..passage.end], passage.text, "{relative}");
        }
    }

    #[test]
    fn a_document_scope_never_returns_another_document() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let tala = id_of(&harness.root, "notes/tala-sa-proyekto.md");
        let (passages, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("project submission deadline", Scope::Document(&tala))
        });
        let PromptEvidence { passages, matched } = passages.unwrap();
        assert!(matched, "the file answers the question");
        assert!(!passages.is_empty());
        assert!(passages.iter().all(|passage| passage.document_id == tala));
    }

    #[test]
    fn a_chosen_file_reaches_the_prompt_even_when_nothing_in_it_matches_the_question() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let budget = id_of(&harness.root, "personal/budget-notes.md");

        // The gated folder search finds nothing for this question...
        assert!(harness.search(&mut embedder, "quantum chromodynamics").is_empty());
        // ...but the file the user chose is still sent, whatever they asked.
        let (passages, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("what does this file say?", Scope::Document(&budget))
        });
        let PromptEvidence { passages, matched } = passages.unwrap();
        // Sent, but marked, so the answer is not taken for a sourced match.
        assert!(!matched, "nothing in the file matched the question");
        assert!(!passages.is_empty());
        assert!(passages.iter().all(|passage| passage.document_id == budget));
        assert!(passages.iter().any(|passage| passage.text.contains("transport")));
        // In reading order, so the model reads the file as written.
        assert!(passages.windows(2).all(|pair| pair[0].start <= pair[1].start));
    }

    #[test]
    fn a_chosen_file_is_sent_in_keyword_mode_too_and_never_from_a_stale_revision() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        embedder.installed = false;
        let plan = id_of(&harness.root, "projects/project-plan.md");
        let (opening, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("zzz nothing matches", Scope::Document(&plan))
        });
        let opening = opening.unwrap();
        assert!(!opening.passages.is_empty(), "its opening passages are sent");
        assert!(!opening.matched, "and marked as not matching");
        let (keyword, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("submission deadline", Scope::Document(&plan))
        });
        assert!(keyword.unwrap().matched, "a keyword hit is a match");

        // The file changes without the scan noticing (same size and mtime).
        let path = harness.folder.path().join("projects/project-plan.md");
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let text = fs::read_to_string(&path).unwrap().replace("October 20", "October 99");
        fs::write(&path, text).unwrap();
        fs::File::options().write(true).open(&path).unwrap().set_modified(modified).unwrap();
        let (during, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("zzz nothing matches", Scope::Document(&plan))
        });
        let during = during.unwrap().passages;
        assert!(!during.is_empty(), "the file was read again and looked at once more");
        assert!(
            during.iter().all(|passage| !passage.text.contains("October 20")),
            "the old revision is never sent"
        );
        assert!(during.iter().any(|passage| passage.text.contains("October 99")));
    }

    #[test]
    fn a_folder_wide_prompt_never_exceeds_the_evidence_budget() {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open_in_memory().unwrap();
        let root = index::tests::authorize(&conn, folder.path());
        for number in 0..12 {
            // Each file is one 1,100-byte paragraph about the project.
            let words = "project plan deadline ".repeat(50);
            fs::write(folder.path().join(format!("plan-{number:02}.md")), format!("# Plan {number}\n\n{words}")).unwrap();
        }
        for number in 0..30 {
            fs::write(folder.path().join(format!("budget-{number:02}.md")), "Budget money spending").unwrap();
        }
        index::tests::scan(&mut conn, &root);
        let mut harness = Harness::from(folder, conn, root);
        let mut embedder = ConceptEmbedder::new("r1");
        let (passages, _) = harness.request(&mut embedder, |index| {
            index.prompt_evidence("project plan deadline", Scope::Folder).map(|evidence| evidence.passages)
        });
        let passages = passages.unwrap();
        let bytes = passages.iter().map(|passage| passage.text.len()).sum::<usize>();
        assert!(!passages.is_empty() && passages.len() <= MAX_PASSAGES);
        assert!(bytes <= grounding::MAX_ANSWER_EVIDENCE_BYTES, "{bytes} bytes");
    }

    #[test]
    fn a_cancelled_fill_keeps_its_committed_batches_and_the_next_request_finishes_the_rest() {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open_in_memory().unwrap();
        let root = index::tests::authorize(&conn, folder.path());
        for number in 0..40 {
            // A quarter are about the project, so the evidence gate has
            // something to stand out from.
            let about = if number % 4 == 0 { "Project plan" } else { "Budget money" };
            fs::write(
                folder.path().join(format!("note-{number:02}.md")),
                format!("{about} number {number}"),
            )
            .unwrap();
        }
        index::tests::scan(&mut conn, &root);
        let mut harness = Harness::from(folder, conn, root);
        let fingerprint =
            embedding_sync::stored_space_fingerprint(&concept_space("r1")).unwrap();

        let mut embedder = ConceptEmbedder::new("r1");
        embedder.cancel_on_batch = Some(2);
        let (cancelled, _) = harness.request(&mut embedder, |index| {
            index.search("project plan", Scope::Folder, 10)
        });
        assert_eq!(cancelled.unwrap_err().code, ErrorCode::Cancelled);
        let (embedded, total) =
            index::embedding_coverage(&harness.conn, &harness.root.id, &fingerprint).unwrap();
        assert_eq!((embedded, total), (folio_core::embeddings::DEFAULT_BATCH_SIZE, 40));

        harness.cancel.store(false, Ordering::Release);
        let mut resumed = ConceptEmbedder::new("r1");
        let results = harness.search(&mut resumed, "project plan");
        assert_eq!(resumed.passages_embedded, 40 - embedded, "only the missing chunks are embedded");
        assert!(!results.is_empty());
    }

    #[test]
    fn a_cancelled_request_stops_waiting_for_a_scan_another_request_holds() {
        let (folder, conn, root) = fixture_workspace();
        let mut conn = conn;
        let (scanning, syncing) = (Mutex::new(()), Mutex::new(()));
        let cancel = AtomicBool::new(true);
        let mut embedder = ConceptEmbedder::new("r1");
        let held = scanning.lock().unwrap();
        let failure: FolioError = {
            let mut sink = |_: PreparingProgress| {};
            let mut index = LocalIndex::new(
                &mut conn, &root, &scanning, &syncing, &mut embedder, &cancel, &mut sink,
            );
            index.refresh_files().unwrap_err()
        };
        drop(held);
        drop(folder);
        assert_eq!(failure.code, ErrorCode::Cancelled);
    }

    #[test]
    fn a_request_with_nothing_left_to_embed_never_waits_for_a_sync_another_task_holds() {
        let (folder, mut conn, root) = fixture_workspace();
        let (scanning, syncing) = (Mutex::new(()), Mutex::new(()));
        let cancel = AtomicBool::new(false);
        let mut sink = |_: PreparingProgress| {};
        let mut first = ConceptEmbedder::new("r1");
        LocalIndex::new(&mut conn, &root, &scanning, &syncing, &mut first, &cancel, &mut sink)
            .prepare()
            .unwrap();

        // A Graph refresh holds the sync lock for its whole run. Only a cancel
        // ends a wait for that lock, so a request that waited would fail.
        let held = syncing.lock().unwrap();
        let mut second = ConceptEmbedder::new("r1");
        let covered = {
            let mut index =
                LocalIndex::new(&mut conn, &root, &scanning, &syncing, &mut second, &cancel, &mut sink);
            index.refresh_files().unwrap();
            cancel.store(true, Ordering::Release);
            index.prepare()
        };
        assert!(covered.is_ok(), "nothing pending, so no wait: {covered:?}");
        assert_eq!(second.passages_embedded, 0);

        // With chunks pending it still waits its turn behind the holder.
        cancel.store(false, Ordering::Release);
        fs::write(folder.path().join("new-budget.md"), "Budget money for the trip").unwrap();
        let mut third = ConceptEmbedder::new("r1");
        let pending = {
            let mut index =
                LocalIndex::new(&mut conn, &root, &scanning, &syncing, &mut third, &cancel, &mut sink);
            index.refresh_files().unwrap();
            cancel.store(true, Ordering::Release);
            index.prepare()
        };
        drop(held);
        drop(folder);
        assert_eq!(pending.unwrap_err().code, ErrorCode::Cancelled);
        assert_eq!(third.passages_embedded, 0);
    }

    #[test]
    fn a_file_the_user_picked_is_read_whatever_the_description_says() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let budget = id_of(&harness.root, "personal/budget-notes.md");
        let (corpus, _) = harness.request(&mut embedder, |index| {
            index.interpretation_corpus(Some("something unrelated"), Some(&budget))
        });
        let corpus = corpus.unwrap();
        assert!(corpus.contents.contains_key(&budget));
        assert!(corpus.chunks.iter().any(|chunk| chunk.document_id == budget));
        assert_eq!(corpus.contents.len(), 1, "nothing else matched");
    }

    #[test]
    fn interpretation_reads_only_the_files_a_target_description_could_mean() {
        let mut harness = Harness::fixtures();
        let mut embedder = ConceptEmbedder::new("r1");
        let (corpus, _) = harness.request(&mut embedder, |index| {
            index.interpretation_corpus(Some("meeting notes"), None)
        });
        let corpus = corpus.unwrap();
        let all = harness.chunk_total();
        assert!(corpus.documents.len() >= 12, "metadata for every indexed document");
        let meeting = id_of(&harness.root, "meetings/meeting-notes.md");
        assert!(corpus.contents.contains_key(&meeting));
        assert!(corpus.contents.len() <= INTERPRETATION_CANDIDATES);
        assert!(corpus.contents.len() < corpus.documents.len(), "not the whole folder");
        assert!(!corpus.chunks.is_empty() && corpus.chunks.len() < all);
        assert!(corpus.chunks.iter().all(|chunk| corpus.contents.contains_key(&chunk.document_id)));
        let record = corpus.documents.iter().find(|document| document.id == meeting).unwrap();
        assert_eq!(
            record.content_hash.as_deref(),
            Some(crate::identity::content_hash(corpus.contents[&meeting].as_bytes()).as_str())
        );
        assert_eq!(embedder.passages_embedded + embedder.queries_embedded, 0, "interpretation embeds nothing");
    }
}
