use std::sync::atomic::{AtomicBool, Ordering};

use folio_core::contracts::EmbeddingSpace as ProviderEmbeddingSpace;
use folio_core::embeddings::stored_chunk_space;
use rusqlite::Connection;
use serde::Serialize;

use crate::db::NativeResult;
use crate::error::{error, ErrorCode, FolioError};
use crate::identity::embedding_space_fingerprint;
use crate::index::{self, ChunkVector, PendingChunk};

/// A single provider batch is small enough to keep the provider and index
/// operation responsive while still using the provider's normal batch size.
pub(crate) const EMBEDDING_BATCH_SIZE: usize = folio_core::embeddings::DEFAULT_BATCH_SIZE;
pub(crate) const MAX_STALE_DROPS: usize = 256;
pub(crate) const MAX_IDLE_PASSES: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct SyncLimits {
    pub batch: usize,
    pub max_stale_drops: usize,
    pub max_idle_passes: usize,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            batch: EMBEDDING_BATCH_SIZE,
            max_stale_drops: MAX_STALE_DROPS,
            max_idle_passes: MAX_IDLE_PASSES,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSyncSummary {
    pub workspace_id: String,
    pub space_fingerprint: String,
    pub stored: usize,
    pub dropped_stale: usize,
    pub cancelled: bool,
    pub complete: bool,
}

/// The narrow persistent-index seam used by the loop. Each method is one
/// short SQLite operation; it never holds a provider lock.
pub trait ChunkStore {
    fn pending(&mut self, limit: usize) -> NativeResult<Vec<PendingChunk>>;
    fn put(&mut self, items: &[ChunkVector]) -> NativeResult<usize>;
}

pub struct IndexChunkStore {
    conn: Connection,
    workspace_id: String,
    fingerprint: String,
}

impl IndexChunkStore {
    pub fn new(conn: Connection, workspace_id: String, fingerprint: String) -> Self {
        Self {
            conn,
            workspace_id,
            fingerprint,
        }
    }
}

impl ChunkStore for IndexChunkStore {
    fn pending(&mut self, limit: usize) -> NativeResult<Vec<PendingChunk>> {
        index::pending_embedding_chunks(&self.conn, &self.workspace_id, &self.fingerprint, limit)
    }

    fn put(&mut self, items: &[ChunkVector]) -> NativeResult<usize> {
        index::put_embeddings(&mut self.conn, &self.workspace_id, &self.fingerprint, items)
    }
}

/// A provider seam for one native batch. The returned space is the space that
/// actually produced the vectors, so a model switch cannot store under the
/// run's original fingerprint.
pub trait PassageEmbedder {
    fn embed_batch(
        &mut self,
        texts: &[String],
        cancel: &AtomicBool,
    ) -> NativeResult<(ProviderEmbeddingSpace, Vec<Vec<f32>>)>;
}

pub(crate) fn stored_index_space(
    provider: &ProviderEmbeddingSpace,
) -> NativeResult<index::EmbeddingSpace> {
    let dimensions = u32::try_from(provider.dimensions).map_err(|_| {
        error(
            ErrorCode::EmbeddingSpaceMismatch,
            "The embedding provider returned too many dimensions for the native index.",
        )
        .with_detail("dimensions", provider.dimensions.to_string())
    })?;
    let stored = stored_chunk_space(provider);
    Ok(index::EmbeddingSpace {
        model_id: stored.model_id,
        revision: stored.revision,
        quantization: stored.quantization,
        dimensions,
        preprocessing_fingerprint: stored.preprocessing_fingerprint,
    })
}

pub(crate) fn stored_space_fingerprint(provider: &ProviderEmbeddingSpace) -> NativeResult<String> {
    let space = stored_index_space(provider)?;
    Ok(embedding_space_fingerprint(
        &space.model_id,
        &space.revision,
        &space.quantization,
        space.dimensions,
        &space.preprocessing_fingerprint,
    ))
}

fn space_mismatch(expected: &str, actual: &str) -> FolioError {
    error(
        ErrorCode::EmbeddingSpaceMismatch,
        "The embedding provider changed spaces while the index was being filled.",
    )
    .with_detail("expected", expected)
    .with_detail("actual", actual)
}

fn invalid_model_output(expected: usize, actual: usize) -> FolioError {
    error(
        ErrorCode::Internal,
        "The embedding provider returned one vector per chunk inconsistently.",
    )
    .with_detail("reportedCode", "invalidModelOutput")
    .with_detail("expectedCount", expected.to_string())
    .with_detail("actualCount", actual.to_string())
}

/// `Some(chunkId)` only for the two native refusals that mean the chunk
/// changed while it was being embedded. Missing or malformed identity details
/// are propagated because guessing would weaken the native contract.
pub(crate) fn stale_chunk_id(error: &FolioError) -> Option<i64> {
    if error.code != ErrorCode::EvidenceInvalid {
        return None;
    }
    if !matches!(
        error.detail("reason"),
        Some("chunkChanged" | "chunkMissing")
    ) {
        return None;
    }
    error.detail("chunkId")?.parse().ok()
}

fn finish(
    summary: &mut EmbeddingSyncSummary,
    stored_this_pass: usize,
    cancelled: bool,
    complete: bool,
) -> EmbeddingSyncSummary {
    summary.stored += stored_this_pass;
    summary.cancelled = cancelled;
    summary.complete = complete;
    summary.clone()
}

/// Fill one registered native embedding space from pending chunks.
pub fn sync_embeddings<S: ChunkStore, E: PassageEmbedder>(
    store: &mut S,
    embedder: &mut E,
    run_space: &ProviderEmbeddingSpace,
    space_fingerprint: &str,
    workspace_id: String,
    cancel: &AtomicBool,
    limits: SyncLimits,
) -> NativeResult<EmbeddingSyncSummary> {
    let expected_space = stored_space_fingerprint(run_space)?;
    if expected_space != space_fingerprint {
        return Err(space_mismatch(&expected_space, space_fingerprint));
    }

    let mut summary = EmbeddingSyncSummary {
        workspace_id,
        space_fingerprint: space_fingerprint.to_owned(),
        stored: 0,
        dropped_stale: 0,
        cancelled: false,
        complete: false,
    };
    let mut idle_passes = 0;

    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(finish(&mut summary, 0, true, false));
        }

        let batch = store.pending(limits.batch.max(1))?;
        if batch.is_empty() {
            return Ok(finish(&mut summary, 0, false, true));
        }

        let texts = batch
            .iter()
            .map(|chunk| chunk.text.clone())
            .collect::<Vec<_>>();
        let (actual_space, vectors) = match embedder.embed_batch(&texts, cancel) {
            Ok(result) => result,
            Err(failure) if failure.code == ErrorCode::Cancelled => {
                return Ok(finish(&mut summary, 0, true, false));
            }
            Err(failure) => return Err(failure),
        };

        // An embedder may notice cancellation after its final provider call.
        // Do not put that in-flight batch after the caller asked us to stop.
        if cancel.load(Ordering::Acquire) {
            return Ok(finish(&mut summary, 0, true, false));
        }

        let actual_fingerprint = stored_space_fingerprint(&actual_space)?;
        if actual_fingerprint != expected_space {
            return Err(space_mismatch(&expected_space, &actual_fingerprint));
        }
        if vectors.len() != batch.len() {
            return Err(invalid_model_output(batch.len(), vectors.len()));
        }

        let mut items = batch
            .into_iter()
            .zip(vectors)
            .map(|(chunk, vector)| ChunkVector {
                chunk_id: chunk.chunk_id,
                // This is the exact hash returned by pending(). It is never
                // recomputed from text, because chunk IDs may be reused.
                content_hash: chunk.content_hash,
                vector,
            })
            .collect::<Vec<_>>();
        let mut stored_this_pass = 0;

        while !items.is_empty() {
            // Check after embedding and before every put, including retries
            // after a stale refusal.
            if cancel.load(Ordering::Acquire) {
                return Ok(finish(&mut summary, stored_this_pass, true, false));
            }
            match store.put(&items) {
                Ok(stored) => {
                    stored_this_pass += stored;
                    break;
                }
                Err(failure) => {
                    let Some(chunk_id) = stale_chunk_id(&failure) else {
                        return Err(failure);
                    };
                    let before = items.len();
                    items.retain(|item| item.chunk_id != chunk_id);
                    if items.len() == before {
                        return Err(failure);
                    }
                    summary.dropped_stale += before - items.len();
                    if summary.dropped_stale >= limits.max_stale_drops {
                        return Ok(finish(&mut summary, stored_this_pass, false, false));
                    }
                }
            }
        }

        summary.stored += stored_this_pass;
        if stored_this_pass == 0 {
            idle_passes += 1;
            if idle_passes >= limits.max_idle_passes {
                return Ok(finish(&mut summary, 0, false, false));
            }
        } else {
            idle_passes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::index;
    use crate::workspace::ScopedRoot;
    use rusqlite::Connection;
    use sha2::{Digest, Sha256};
    use std::collections::VecDeque;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const SENTINEL_HASH: &str =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    fn provider_space(revision: &str) -> ProviderEmbeddingSpace {
        ProviderEmbeddingSpace {
            model_id: "test-embedding".into(),
            revision: revision.into(),
            quantization: "test".into(),
            dimensions: 8,
            preprocessing_fingerprint: "provider-input-v1".into(),
        }
    }

    fn pending_chunk(id: i64, text: &str, hash: &str) -> PendingChunk {
        PendingChunk {
            chunk_id: id,
            document_id: format!("workspace:{id}.md"),
            text: text.into(),
            content_hash: hash.into(),
        }
    }

    fn fake_vector(text: &str) -> Vec<f32> {
        let digest = Sha256::digest(text.as_bytes());
        let values = digest[..8]
            .iter()
            .map(|byte| (*byte as f32 / 127.5) - 1.0)
            .collect::<Vec<_>>();
        let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
        values.into_iter().map(|value| value / norm).collect()
    }

    struct ScriptedStore {
        pending: VecDeque<Vec<PendingChunk>>,
        puts: Vec<Vec<ChunkVector>>,
        put_results: VecDeque<NativeResult<usize>>,
        cancel_on_failure: Option<Arc<AtomicBool>>,
    }

    impl ScriptedStore {
        fn new(pending: Vec<Vec<PendingChunk>>) -> Self {
            Self {
                pending: pending.into(),
                puts: Vec::new(),
                put_results: VecDeque::new(),
                cancel_on_failure: None,
            }
        }

        fn cancel_on_failure(&mut self, cancel: Arc<AtomicBool>) {
            self.cancel_on_failure = Some(cancel);
        }
    }

    impl ChunkStore for ScriptedStore {
        fn pending(&mut self, _limit: usize) -> NativeResult<Vec<PendingChunk>> {
            Ok(self.pending.pop_front().unwrap_or_default())
        }

        fn put(&mut self, items: &[ChunkVector]) -> NativeResult<usize> {
            self.puts.push(items.to_vec());
            let result = self
                .put_results
                .pop_front()
                .unwrap_or_else(|| Ok(items.len()));
            if result.is_err() {
                if let Some(cancel) = &self.cancel_on_failure {
                    cancel.store(true, Ordering::Release);
                }
            }
            result
        }
    }

    struct RecordingEmbedder {
        space: ProviderEmbeddingSpace,
        inputs: Vec<Vec<String>>,
        vectors: Option<Vec<Vec<f32>>>,
        calls: usize,
        cancel_on_call: Option<usize>,
        cancel_after_embed_call: Option<usize>,
        busy_on_call: Option<usize>,
        mismatch_on_call: Option<usize>,
    }

    impl RecordingEmbedder {
        fn new(space: ProviderEmbeddingSpace) -> Self {
            Self {
                space,
                inputs: Vec::new(),
                vectors: None,
                calls: 0,
                cancel_on_call: None,
                cancel_after_embed_call: None,
                busy_on_call: None,
                mismatch_on_call: None,
            }
        }
    }

    impl PassageEmbedder for RecordingEmbedder {
        fn embed_batch(
            &mut self,
            texts: &[String],
            cancel: &AtomicBool,
        ) -> NativeResult<(ProviderEmbeddingSpace, Vec<Vec<f32>>)> {
            self.calls += 1;
            self.inputs.push(texts.to_vec());
            if self.busy_on_call == Some(self.calls) {
                return Err(
                    error(ErrorCode::ProviderBusy, "Model Lab is measuring models.")
                        .with_detail("reason", "modelLabRunning"),
                );
            }
            if self.cancel_on_call == Some(self.calls) {
                cancel.store(true, Ordering::Release);
                return Err(error(ErrorCode::Cancelled, "Embedding cancelled."));
            }
            let space = if self.mismatch_on_call == Some(self.calls) {
                provider_space("different-revision")
            } else {
                self.space.clone()
            };
            let vectors = self
                .vectors
                .clone()
                .unwrap_or_else(|| texts.iter().map(|text| fake_vector(text)).collect());
            if self.cancel_after_embed_call == Some(self.calls) {
                cancel.store(true, Ordering::Release);
            }
            Ok((space, vectors))
        }
    }

    fn run_space_fingerprint(space: &ProviderEmbeddingSpace) -> String {
        stored_space_fingerprint(space).unwrap()
    }

    fn sync_default<S: ChunkStore, E: PassageEmbedder>(
        store: &mut S,
        embedder: &mut E,
        space: &ProviderEmbeddingSpace,
        workspace_id: &str,
        cancel: &AtomicBool,
    ) -> NativeResult<EmbeddingSyncSummary> {
        sync_embeddings(
            store,
            embedder,
            space,
            &run_space_fingerprint(space),
            workspace_id.to_owned(),
            cancel,
            SyncLimits::default(),
        )
    }

    #[test]
    fn echoes_pending_hash_verbatim_and_embeds_exact_chunk_text() {
        let space = provider_space("r1");
        let chunk = pending_chunk(1, "Filipino: Oktubre 20 — Señora", SENTINEL_HASH);
        let mut store = ScriptedStore::new(vec![vec![chunk], vec![]]);
        let mut embedder = RecordingEmbedder::new(space.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.complete);
        assert_eq!(
            embedder.inputs,
            vec![vec![String::from("Filipino: Oktubre 20 — Señora")]]
        );
        assert_eq!(store.puts[0][0].content_hash, SENTINEL_HASH);
    }

    #[test]
    fn stale_refusal_drops_only_the_named_item_and_retries_the_rest() {
        let space = provider_space("r1");
        let first = pending_chunk(1, "one", "sha256:one");
        let second = pending_chunk(2, "two", "sha256:two");
        let mut store = ScriptedStore::new(vec![vec![first, second], vec![]]);
        store
            .put_results
            .push_back(Err(error(ErrorCode::EvidenceInvalid, "stale")
                .with_detail("reason", "chunkChanged")
                .with_detail("chunkId", "1")));
        let mut embedder = RecordingEmbedder::new(space.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.complete);
        assert_eq!(summary.stored, 1);
        assert_eq!(summary.dropped_stale, 1);
        assert_eq!(store.puts.len(), 2);
        assert_eq!(store.puts[1].len(), 1);
        assert_eq!(store.puts[1][0].chunk_id, 2);
        assert_eq!(embedder.calls, 1);
        assert_eq!(store.puts[1][0].vector, store.puts[0][1].vector);
    }

    #[test]
    fn other_evidence_invalid_and_missing_identity_are_reported() {
        for failure in [
            error(ErrorCode::EvidenceInvalid, "other").with_detail("reason", "other"),
            error(ErrorCode::EvidenceInvalid, "missing id").with_detail("reason", "chunkChanged"),
        ] {
            let space = provider_space("r1");
            let chunk = pending_chunk(1, "one", "sha256:one");
            let mut store = ScriptedStore::new(vec![vec![chunk]]);
            store.put_results.push_back(Err(failure.clone()));
            let mut embedder = RecordingEmbedder::new(space.clone());
            let actual = sync_default(
                &mut store,
                &mut embedder,
                &space,
                "workspace",
                &AtomicBool::new(false),
            )
            .unwrap_err();
            assert_eq!(actual, failure);
        }
    }

    #[test]
    fn model_space_mismatch_is_reported_before_store() {
        let space = provider_space("r1");
        let chunk = pending_chunk(1, "one", "sha256:one");
        let mut store = ScriptedStore::new(vec![vec![chunk]]);
        let mut embedder = RecordingEmbedder::new(space.clone());
        embedder.mismatch_on_call = Some(1);
        let actual = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap_err();

        assert_eq!(actual.code, ErrorCode::EmbeddingSpaceMismatch);
        assert!(store.puts.is_empty());
    }

    #[test]
    fn cancellation_after_a_committed_batch_keeps_that_batch_only() {
        let space = provider_space("r1");
        let first = pending_chunk(1, "one", "sha256:one");
        let second = pending_chunk(2, "two", "sha256:two");
        let mut store = ScriptedStore::new(vec![vec![first], vec![second]]);
        let mut embedder = RecordingEmbedder::new(space.clone());
        embedder.cancel_on_call = Some(2);
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.cancelled);
        assert!(!summary.complete);
        assert_eq!(summary.stored, 1);
        assert_eq!(store.puts.len(), 1);
    }

    #[test]
    fn continuously_stale_chunks_stop_after_the_idle_bound() {
        let space = provider_space("r1");
        let mut store = ScriptedStore::new(Vec::new());
        store.pending =
            std::iter::repeat_with(|| vec![pending_chunk(1, "rewritten", "sha256:rewritten")])
                .take(MAX_IDLE_PASSES + 1)
                .collect();
        store.put_results = std::iter::repeat_with(|| {
            Err(error(ErrorCode::EvidenceInvalid, "stale")
                .with_detail("reason", "chunkChanged")
                .with_detail("chunkId", "1"))
        })
        .take(MAX_IDLE_PASSES + 1)
        .collect();
        let mut embedder = RecordingEmbedder::new(space.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(!summary.complete);
        assert_eq!(summary.dropped_stale, MAX_IDLE_PASSES);
        assert_eq!(store.puts.len(), MAX_IDLE_PASSES);
    }

    #[test]
    fn cancel_after_embedding_before_put_stores_nothing_from_that_batch() {
        let space = provider_space("r1");
        let first = pending_chunk(1, "one", "sha256:one");
        let second = pending_chunk(2, "two", "sha256:two");
        let mut store = ScriptedStore::new(vec![vec![first], vec![second]]);
        let mut embedder = RecordingEmbedder::new(space.clone());
        embedder.cancel_after_embed_call = Some(2);
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.cancelled);
        assert!(!summary.complete);
        assert_eq!(summary.stored, 1);
        assert_eq!(store.puts.len(), 1);
    }

    #[test]
    fn cancel_between_stale_retries_stops_before_next_put() {
        let space = provider_space("r1");
        let first = pending_chunk(1, "one", "sha256:one");
        let second = pending_chunk(2, "two", "sha256:two");
        let third = pending_chunk(3, "three", "sha256:three");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut store = ScriptedStore::new(vec![vec![first], vec![second, third]]);
        store.put_results.push_back(Ok(1));
        store
            .put_results
            .push_back(Err(error(ErrorCode::EvidenceInvalid, "stale")
                .with_detail("reason", "chunkChanged")
                .with_detail("chunkId", "2")));
        store.cancel_on_failure(cancel.clone());
        let mut embedder = RecordingEmbedder::new(space.clone());
        let summary =
            sync_default(&mut store, &mut embedder, &space, "workspace", &cancel).unwrap();

        assert!(summary.cancelled);
        assert_eq!(summary.stored, 1);
        assert_eq!(summary.dropped_stale, 1);
        assert_eq!(store.puts.len(), 2);
    }

    #[test]
    fn stale_cap_exit_reports_cumulative_counters() {
        let space = provider_space("r1");
        let mut store = ScriptedStore::new(vec![
            vec![
                pending_chunk(1, "one", "sha256:one"),
                pending_chunk(2, "two", "sha256:two"),
            ],
            vec![
                pending_chunk(3, "three", "sha256:three"),
                pending_chunk(4, "four", "sha256:four"),
            ],
            vec![pending_chunk(5, "five", "sha256:five")],
        ]);
        store.put_results.push_back(Ok(2));
        for (id, message) in [(3, "three stale"), (4, "four stale"), (5, "five stale")] {
            store
                .put_results
                .push_back(Err(error(ErrorCode::EvidenceInvalid, message)
                    .with_detail("reason", "chunkChanged")
                    .with_detail("chunkId", id.to_string())));
        }
        let mut embedder = RecordingEmbedder::new(space.clone());
        let summary = sync_embeddings(
            &mut store,
            &mut embedder,
            &space,
            &run_space_fingerprint(&space),
            "workspace".into(),
            &AtomicBool::new(false),
            SyncLimits {
                batch: 2,
                max_stale_drops: 3,
                max_idle_passes: 100,
            },
        )
        .unwrap();

        assert_eq!(summary.stored, 2);
        assert_eq!(summary.dropped_stale, 3);
        assert!(!summary.complete);
        assert!(!summary.cancelled);
        assert_eq!(store.puts.len(), 4);
    }

    struct SqliteStore {
        conn: Connection,
        root: ScopedRoot,
        fingerprint: String,
        after_pending: Option<Box<dyn FnOnce(&mut Connection, &ScopedRoot)>>,
        refusals: Vec<FolioError>,
    }

    impl ChunkStore for SqliteStore {
        fn pending(&mut self, limit: usize) -> NativeResult<Vec<PendingChunk>> {
            let pending = index::pending_embedding_chunks(
                &self.conn,
                &self.root.id,
                &self.fingerprint,
                limit,
            )?;
            if let Some(hook) = self.after_pending.take() {
                hook(&mut self.conn, &self.root);
            }
            Ok(pending)
        }

        fn put(&mut self, items: &[ChunkVector]) -> NativeResult<usize> {
            let result =
                index::put_embeddings(&mut self.conn, &self.root.id, &self.fingerprint, items);
            if let Err(failure) = &result {
                self.refusals.push(failure.clone());
            }
            result
        }
    }

    fn sqlite_fixture(
        files: &[(&str, &str)],
    ) -> (
        tempfile::TempDir,
        Connection,
        ScopedRoot,
        String,
        ProviderEmbeddingSpace,
    ) {
        let folder = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = folder.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let mut conn = db::open_in_memory().unwrap();
        let root = index::tests::authorize(&conn, folder.path());
        index::tests::scan(&mut conn, &root);
        let provider = provider_space("r1");
        let native = stored_index_space(&provider).unwrap();
        let fingerprint = index::register_space(&conn, &native).unwrap();
        (folder, conn, root, fingerprint, provider)
    }

    fn assert_current_vector_and_old_absent(
        conn: &Connection,
        root: &ScopedRoot,
        fingerprint: &str,
        old_text: &str,
        new_text: &str,
    ) {
        let current_hits =
            index::vector_candidates(conn, &root.id, fingerprint, &fake_vector(new_text), 100)
                .unwrap();
        let current = current_hits
            .iter()
            .find(|candidate| candidate.passage.text == new_text)
            .expect("the current chunk has a vector");
        assert!((current.score - 1.0).abs() < 0.0001);

        let old_hits =
            index::vector_candidates(conn, &root.id, fingerprint, &fake_vector(old_text), 100)
                .unwrap();
        assert!(!old_hits
            .iter()
            .any(|candidate| candidate.passage.text == new_text && candidate.score >= 0.99));
    }

    #[test]
    fn edit_between_fetch_and_store_reuses_chunk_id_for_each_language() {
        let cases = [
            (
                "english.md",
                "Project plan: the deadline is October 20.",
                "Project plan revised: the deadline is now October 23 after the review.",
            ),
            (
                "filipino.md",
                "Plano ng proyekto: ang huling araw ay sa Oktubre 20. Señora, salamat!",
                "Plano ng proyekto: ang bagong huling araw ay sa Oktubre 23. Señora, maraming salamat!",
            ),
            (
                "taglish.md",
                "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.",
                "Hanapin yung revised project plan at palitan ang deadline na October 23 to October 25 pagkatapos ng meeting.",
            ),
        ];

        for (path, old_text, new_text) in cases {
            let (folder, conn, root, fingerprint, provider) = sqlite_fixture(&[(path, old_text)]);
            let old_chunk = index::pending_embedding_chunks(&conn, &root.id, &fingerprint, 10)
                .unwrap()
                .pop()
                .unwrap();
            let file = folder.path().join(path);
            let root_for_hook = root.clone();
            let mut store = SqliteStore {
                conn,
                root: root.clone(),
                fingerprint: fingerprint.clone(),
                after_pending: Some(Box::new(move |conn, _root| {
                    fs::write(&file, new_text).unwrap();
                    index::tests::scan(conn, &root_for_hook);
                })),
                refusals: Vec::new(),
            };
            let mut embedder = RecordingEmbedder::new(provider.clone());
            let summary = sync_default(
                &mut store,
                &mut embedder,
                &provider,
                &root.id,
                &AtomicBool::new(false),
            )
            .unwrap();

            let reused: i64 = store
                .conn
                .query_row(
                    "SELECT count(*) FROM chunks WHERE chunk_id = ?1",
                    [old_chunk.chunk_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(reused, 1, "the chunk id must be reused in {path}");
            assert!(summary.complete);
            assert_eq!(summary.stored, 1);
            assert_eq!(summary.dropped_stale, 1);
            assert!(
                index::pending_embedding_chunks(&store.conn, &root.id, &fingerprint, 100,)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(store.refusals.len(), 1);
            assert_eq!(store.refusals[0].detail("reason"), Some("chunkChanged"));
            assert_eq!(
                store.refusals[0]
                    .detail("chunkId")
                    .and_then(|value| value.parse::<i64>().ok()),
                Some(old_chunk.chunk_id)
            );
            assert_current_vector_and_old_absent(
                &store.conn,
                &root,
                &fingerprint,
                old_text,
                new_text,
            );
        }
    }

    #[test]
    fn edit_between_fetch_and_store_reembeds_a_combined_stale_batch() {
        let old = [
            ("english.md", "Project plan: the deadline is October 20."),
            (
                "filipino.md",
                "Plano ng proyekto: ang huling araw ay sa Oktubre 20. Señora, salamat!",
            ),
            (
                "taglish.md",
                "Hanapin yung project plan at palitan ang deadline na October 20 to October 23.",
            ),
        ];
        let new = [
            (
                "english.md",
                "Project plan revised: the deadline is now October 23 after the review.",
            ),
            (
                "filipino.md",
                "Plano ng proyekto: ang bagong huling araw ay sa Oktubre 23. Señora, maraming salamat!",
            ),
            (
                "taglish.md",
                "Hanapin yung revised project plan at palitan ang deadline na October 23 to October 25 pagkatapos ng meeting.",
            ),
        ];
        let (folder, conn, root, fingerprint, provider) = sqlite_fixture(&old);
        let hook_files = new
            .iter()
            .map(|(path, text)| (folder.path().join(path), *text))
            .collect::<Vec<_>>();
        let root_for_hook = root.clone();
        let mut store = SqliteStore {
            conn,
            root: root.clone(),
            fingerprint: fingerprint.clone(),
            after_pending: Some(Box::new(move |conn, _root| {
                for (path, text) in &hook_files {
                    fs::write(path, text).unwrap();
                }
                index::tests::scan(conn, &root_for_hook);
            })),
            refusals: Vec::new(),
        };
        let mut embedder = RecordingEmbedder::new(provider.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &provider,
            &root.id,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.complete);
        assert_eq!(summary.stored, 3);
        assert_eq!(summary.dropped_stale, 3);
        assert_eq!(store.refusals.len(), 3);
        assert!(store.refusals.iter().all(|failure| matches!(
            failure.detail("reason"),
            Some("chunkChanged" | "chunkMissing")
        )));
        assert!(
            index::pending_embedding_chunks(&store.conn, &root.id, &fingerprint, 100)
                .unwrap()
                .is_empty()
        );
        for ((_, old_text), (_, new_text)) in old.iter().zip(new.iter()) {
            assert_current_vector_and_old_absent(
                &store.conn,
                &root,
                &fingerprint,
                old_text,
                new_text,
            );
        }
    }

    #[test]
    fn deleted_between_fetch_and_store_is_chunk_missing() {
        let (folder, conn, root, fingerprint, provider) = sqlite_fixture(&[
            ("a.md", "English document to delete."),
            ("b.md", "Filipino document na mananatili."),
        ]);
        let deleted = folder.path().join("a.md");
        let root_for_hook = root.clone();
        let mut store = SqliteStore {
            conn,
            root: root.clone(),
            fingerprint: fingerprint.clone(),
            after_pending: Some(Box::new(move |conn, _root| {
                fs::remove_file(deleted).unwrap();
                index::tests::scan(conn, &root_for_hook);
            })),
            refusals: Vec::new(),
        };
        let mut embedder = RecordingEmbedder::new(provider.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &provider,
            &root.id,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.complete);
        assert_eq!(summary.stored, 1);
        assert_eq!(summary.dropped_stale, 1);
        assert_eq!(store.refusals.len(), 1);
        assert_eq!(store.refusals[0].detail("reason"), Some("chunkMissing"));
        assert!(
            index::pending_embedding_chunks(&store.conn, &root.id, &fingerprint, 100)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn nfc_to_nfd_rewrite_is_a_change() {
        let old_text = "Señora salamat sa plano.";
        let new_text = "Sen\u{0303}ora salamat sa plano.";
        let (folder, conn, root, fingerprint, provider) =
            sqlite_fixture(&[("filipino.md", old_text)]);
        let file = folder.path().join("filipino.md");
        let root_for_hook = root.clone();
        let mut store = SqliteStore {
            conn,
            root: root.clone(),
            fingerprint: fingerprint.clone(),
            after_pending: Some(Box::new(move |conn, _root| {
                fs::write(&file, new_text).unwrap();
                index::tests::scan(conn, &root_for_hook);
            })),
            refusals: Vec::new(),
        };
        let mut embedder = RecordingEmbedder::new(provider.clone());
        let summary = sync_default(
            &mut store,
            &mut embedder,
            &provider,
            &root.id,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(summary.complete);
        assert_eq!(summary.dropped_stale, 1);
        assert_eq!(store.refusals[0].detail("reason"), Some("chunkChanged"));
        assert_current_vector_and_old_absent(&store.conn, &root, &fingerprint, old_text, new_text);
    }

    #[test]
    fn dimension_mismatch_from_store_is_reported() {
        let (folder, conn, root, fingerprint, provider) =
            sqlite_fixture(&[("a.md", "A document with an eight-dimensional space.")]);
        let mut store = SqliteStore {
            conn,
            root: root.clone(),
            fingerprint: fingerprint.clone(),
            after_pending: None,
            refusals: Vec::new(),
        };
        let mut embedder = RecordingEmbedder::new(provider.clone());
        embedder.vectors = Some(vec![vec![0.0; 7]]);
        let failure = sync_default(
            &mut store,
            &mut embedder,
            &provider,
            &root.id,
            &AtomicBool::new(false),
        )
        .unwrap_err();

        assert_eq!(failure.code, ErrorCode::EmbeddingSpaceMismatch);
        let embeddings: i64 = store
            .conn
            .query_row("SELECT count(*) FROM embeddings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(embeddings, 0);
        drop(folder);
    }

    #[test]
    fn unregistered_space_is_reported() {
        let (_folder, conn, root, _fingerprint, _provider) =
            sqlite_fixture(&[("a.md", "A document.")]);
        let mut store = IndexChunkStore::new(conn, root.id, "missing-space".into());
        let failure = store.pending(10).unwrap_err();
        assert_eq!(failure.code, ErrorCode::EmbeddingSpaceMismatch);
    }

    #[test]
    fn model_lab_busy_from_embedder_keeps_prior_batch_and_propagates() {
        let space = provider_space("r1");
        let first = pending_chunk(1, "one", "sha256:one");
        let second = pending_chunk(2, "two", "sha256:two");
        let mut store = ScriptedStore::new(vec![vec![first], vec![second]]);
        let mut embedder = RecordingEmbedder::new(space.clone());
        embedder.busy_on_call = Some(2);
        let failure = sync_default(
            &mut store,
            &mut embedder,
            &space,
            "workspace",
            &AtomicBool::new(false),
        )
        .unwrap_err();

        assert_eq!(failure.code, ErrorCode::ProviderBusy);
        assert_eq!(failure.detail("reason"), Some("modelLabRunning"));
        assert_eq!(store.puts.len(), 1);
    }

    #[test]
    fn immediate_put_turns_concurrent_rescan_into_chunk_changed() {
        use rusqlite::TransactionBehavior;
        use std::thread;
        use std::time::Duration;

        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("a.md"), "old text").unwrap();
        let data = tempfile::tempdir().unwrap();
        let database = data.path().join("folio.sqlite");
        let (workspace_id, chunk, fingerprint) = {
            let mut conn = db::open(&database).unwrap();
            let root = index::tests::authorize(&conn, folder.path());
            index::tests::scan(&mut conn, &root);
            let provider = provider_space("r1");
            let fingerprint =
                index::register_space(&conn, &stored_index_space(&provider).unwrap()).unwrap();
            let chunk = index::pending_embedding_chunks(&conn, &root.id, &fingerprint, 10)
                .unwrap()
                .pop()
                .unwrap();
            (root.id, chunk, fingerprint)
        };
        let mut conn_a = db::open(&database).unwrap();
        let mut conn_b = db::open(&database).unwrap();
        let tx = conn_b
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute(
            "UPDATE chunks SET chunk_text = ?1, content_hash = ?2 WHERE chunk_id = ?3",
            ("new text", "sha256:new-text", chunk.chunk_id),
        )
        .unwrap();

        let workspace_for_thread = workspace_id.clone();
        let fingerprint_for_thread = fingerprint.clone();
        let old_hash = chunk.content_hash.clone();
        let chunk_id = chunk.chunk_id;
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(0);
        let handle = thread::spawn(move || {
            started_tx.send(()).unwrap();
            index::put_embeddings(
                &mut conn_a,
                &workspace_for_thread,
                &fingerprint_for_thread,
                &[ChunkVector {
                    chunk_id,
                    content_hash: old_hash,
                    vector: fake_vector("old text"),
                }],
            )
        });
        started_rx.recv().unwrap();
        thread::sleep(Duration::from_millis(50));
        tx.commit().unwrap();
        let failure = handle.join().unwrap().unwrap_err();

        assert_eq!(failure.code, ErrorCode::EvidenceInvalid);
        assert_eq!(failure.detail("reason"), Some("chunkChanged"));
        assert_eq!(
            failure.detail("chunkId"),
            Some(chunk_id.to_string().as_str())
        );
    }

    #[test]
    fn stored_vectors_use_the_derived_space() {
        let provider = provider_space("r1");
        let stored = stored_chunk_space(&provider);
        assert_ne!(
            stored.preprocessing_fingerprint,
            provider.preprocessing_fingerprint
        );
        assert_eq!(
            stored_space_fingerprint(&provider).unwrap(),
            run_space_fingerprint(&provider)
        );
    }
}
