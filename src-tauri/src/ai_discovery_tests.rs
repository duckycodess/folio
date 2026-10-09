//! Progressive discovery: admission, ownership, tiles, fairness, resume and
//! the completeness proof, against a real SQLite index with synthetic chunks
//! and vectors.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{params, Connection, Transaction, TransactionBehavior};

use crate::ai_discovery::{
    admit_eligible, coverage, run_discovery, CoverageState, DiscoveryLimits, RunContext, RunEnd,
    RunSummary,
};
use crate::db;
use crate::index::{self, EmbeddingSpace};

const WORKSPACE: &str = "w";

struct Fixture {
    conn: Connection,
    space: String,
    versions: std::collections::BTreeMap<String, u32>,
}

/// A chunk's text and 2-D vector. Angles are small so most pairs are similar.
type Chunk = (String, [f32; 2]);

fn chunks(count: usize, name: &str, angle: f32) -> Vec<Chunk> {
    (0..count)
        .map(|index| {
            let a = angle + index as f32 * 0.01;
            (
                format!("{name} paragraph {index} about the shared project."),
                [a.cos(), a.sin()],
            )
        })
        .collect()
}

impl Fixture {
    fn new() -> Self {
        let conn = db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, root_path, authorized_at) VALUES (?1, '/w', '0')",
            [WORKSPACE],
        )
        .unwrap();
        let space = register(&conn, "r1");
        Self {
            conn,
            space,
            versions: Default::default(),
        }
    }

    fn hash(&self, name: &str) -> String {
        format!(
            "sha256:{name}:{}",
            self.versions.get(name).copied().unwrap_or(0)
        )
    }

    /// Adds (or, for an edit, replaces) a document, optionally embedding its
    /// chunks in the fixture's space.
    fn put(&mut self, name: &str, chunks: &[Chunk], embedded: bool) {
        let version = self.versions.entry(name.to_owned()).or_insert(0);
        *version += 1;
        let hash = format!("sha256:{name}:{version}");
        let exists: bool = self
            .conn
            .query_row(
                "SELECT count(*) FROM documents WHERE id = ?1",
                [name],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            > 0;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        if exists {
            index::clear_derived(&tx, name).unwrap();
            tx.execute(
                "UPDATE documents SET content_hash = ?2 WHERE id = ?1",
                params![name, hash],
            )
            .unwrap();
        } else {
            tx.execute(
                "INSERT INTO documents (id, workspace_id, relative_path, content_hash, media_type, size_bytes, modified_at, name) VALUES (?1, ?2, ?3, ?4, 'text/markdown', 1, '0', ?1)",
                params![name, WORKSPACE, format!("{name}.md"), hash],
            )
            .unwrap();
        }
        let mut offset = 0usize;
        for (ordinal, (text, vector)) in chunks.iter().enumerate() {
            tx.execute(
                "INSERT INTO chunks (document_id, ordinal, chunk_text, start_offset, end_offset, page, content_hash) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
                params![name, ordinal as i64, text, offset as i64, (offset + text.len()) as i64, format!("c:{text}")],
            )
            .unwrap();
            let chunk_id = tx.last_insert_rowid();
            offset += text.len() + 1;
            if embedded {
                embed(&tx, chunk_id, &self.space, vector);
            }
        }
        tx.commit().unwrap();
    }

    /// Embeds every chunk of a document that has no vector in the space yet.
    fn embed_pending(&mut self, name: &str, chunks: &[Chunk]) {
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT chunk_id FROM chunks WHERE document_id = ?1 ORDER BY ordinal")
            .unwrap()
            .query_map([name], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let tx = self.conn.transaction().unwrap();
        for (chunk_id, (_, vector)) in ids.iter().zip(chunks) {
            embed(&tx, *chunk_id, &self.space, vector);
        }
        tx.commit().unwrap();
    }

    fn remove(&self, name: &str) {
        self.conn
            .execute("DELETE FROM documents WHERE id = ?1", [name])
            .unwrap();
    }

    fn run(&mut self, limits: DiscoveryLimits) -> RunSummary {
        self.run_with(limits, &AtomicBool::new(false), &|_| Ok(true), &mut |_| {})
    }

    fn run_with(
        &mut self,
        limits: DiscoveryLimits,
        cancel: &AtomicBool,
        still_active: &dyn Fn(&Connection) -> crate::db::NativeResult<bool>,
        progress: &mut dyn FnMut(&crate::ai_discovery::DiscoveryProgress),
    ) -> RunSummary {
        run_discovery(
            &mut self.conn,
            &RunContext {
                workspace_id: WORKSPACE,
                space: &self.space,
                limits,
                cancel,
                still_active,
            },
            progress,
        )
        .unwrap()
    }

    /// Runs until nothing is left, returning every run's summary.
    fn run_to_complete(&mut self, limits: DiscoveryLimits) -> Vec<RunSummary> {
        let mut runs = Vec::new();
        loop {
            let summary = self.run(limits);
            let done = summary.end == RunEnd::Complete;
            runs.push(summary);
            assert!(runs.len() < 10_000, "discovery must terminate");
            if done {
                return runs;
            }
        }
    }

    fn coverage(&self) -> crate::ai_discovery::RelationshipCoverage {
        coverage(&self.conn, WORKSPACE, Some(&self.space)).unwrap()
    }

    fn rows(&self, space: &str) -> Vec<(String, String, String, String, u32)> {
        self.conn
            .prepare("SELECT source_document_id, target_document_id, relationship_type, evidence_json, CAST(discovery_cosine * 1000000 AS INTEGER) FROM relationships WHERE space_fingerprint = ?1 ORDER BY id")
            .unwrap()
            .query_map([space], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn seq(&self, name: &str) -> i64 {
        self.conn
            .query_row(
                "SELECT seq FROM ai_relationship_coverage WHERE document_id = ?1 AND space_id = ?2",
                params![name, self.space],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn progress_rows(&self) -> Vec<(String, String, i64, i64)> {
        self.conn
            .prepare("SELECT document_id, partner_id, next_left, next_right FROM ai_pair_progress ORDER BY document_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
}

fn register(conn: &Connection, revision: &str) -> String {
    index::register_space(
        conn,
        &EmbeddingSpace {
            model_id: "m".into(),
            revision: revision.into(),
            quantization: "q".into(),
            dimensions: 2,
            preprocessing_fingerprint: "p".into(),
        },
    )
    .unwrap()
}

fn embed(tx: &Transaction<'_>, chunk_id: i64, space: &str, vector: &[f32; 2]) {
    let blob: Vec<u8> = vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    tx.execute(
        "INSERT OR REPLACE INTO embeddings (chunk_id, space_id, vector) VALUES (?1, ?2, ?3)",
        params![chunk_id, space, blob],
    )
    .unwrap();
}

fn small_tiles() -> DiscoveryLimits {
    DiscoveryLimits {
        tile_rows: 2,
        tile_cols: 2,
        max_run_work: 4,
        tiles_per_job_per_turn: 1,
    }
}

fn comparisons(runs: &[RunSummary]) -> usize {
    runs.iter().map(|run| run.progress.comparisons).sum()
}

fn pairs(runs: &[RunSummary]) -> usize {
    runs.iter().map(|run| run.progress.pairs_completed).sum()
}

#[test]
fn seq_is_never_reused_after_deleting_the_highest_document() {
    let mut fixture = Fixture::new();
    for (index, name) in ["a", "b", "c"].iter().enumerate() {
        fixture.put(name, &chunks(2, name, index as f32 * 0.02), true);
    }
    assert_eq!(
        admit_eligible(&mut fixture.conn, WORKSPACE, &fixture.space.clone(), 1).unwrap(),
        3
    );
    assert_eq!(
        (fixture.seq("a"), fixture.seq("b"), fixture.seq("c")),
        (1, 2, 3)
    );
    fixture.remove("c");
    fixture.put("d", &chunks(2, "d", 0.1), true);
    admit_eligible(&mut fixture.conn, WORKSPACE, &fixture.space.clone(), 2).unwrap();
    assert_eq!(
        fixture.seq("d"),
        4,
        "the deleted seq 3 is not handed out again"
    );
}

#[test]
fn a_new_or_edited_document_flips_coverage_to_partial_and_only_its_own_comparisons_run() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(3, "a", 0.00), true);
    fixture.put("b", &chunks(2, "b", 0.02), true);
    fixture.put("c", &chunks(4, "c", 0.04), true);
    assert_eq!(
        fixture.coverage().state,
        CoverageState::Partial,
        "nothing has run yet"
    );
    let first = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(
        comparisons(&first),
        2 * 3 + 4 * 3 + 4 * 2,
        "every pair once, job rows times partner columns"
    );
    assert_eq!(pairs(&first), 3);
    let done = fixture.coverage();
    assert_eq!(
        (done.state, done.pairs_remaining, done.pairs_considered),
        (CoverageState::Complete, 0, 3)
    );
    assert!(
        !fixture.rows(&fixture.space).is_empty(),
        "similar documents produced candidates"
    );

    fixture.put("d", &chunks(5, "d", 0.06), true);
    assert_eq!(
        fixture.coverage().state,
        CoverageState::Partial,
        "a new document makes the folder partial"
    );
    assert_eq!(
        fixture.coverage().pairs_remaining,
        3,
        "d against the three admitted documents"
    );
    let second = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(
        comparisons(&second),
        5 * (3 + 2 + 4),
        "only d is compared, against each other document"
    );
    assert_eq!(fixture.coverage().state, CoverageState::Complete);

    // An edit re-admits the document with a larger seq and a job of its own.
    fixture.put("b", &chunks(2, "b-edited", 0.03), true);
    let after_edit = fixture.coverage();
    assert_eq!(after_edit.state, CoverageState::Partial);
    let third = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(
        comparisons(&third),
        2 * (3 + 4 + 5),
        "the edited revision against every other document, nothing else"
    );
    assert!(
        fixture.seq("b") > fixture.seq("d"),
        "the edited revision got a fresh, larger seq"
    );
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
}

#[test]
fn documents_embedded_later_are_each_compared_with_every_earlier_one_exactly_once() {
    let mut fixture = Fixture::new();
    let all = [("a", 0.00), ("b", 0.02), ("c", 0.04), ("d", 0.06)];
    for (name, angle) in all {
        fixture.put(name, &chunks(2, name, angle), matches!(name, "a" | "b"));
    }
    let first = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(pairs(&first), 1, "only a and b exist in the space");
    let partial = fixture.coverage();
    assert_eq!(
        (
            partial.state,
            partial.indexed_documents,
            partial.eligible_documents
        ),
        (CoverageState::EmbeddingIncomplete, 4, 2)
    );

    for (name, angle) in &all[2..] {
        fixture.embed_pending(name, &chunks(2, name, *angle));
    }
    let second = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(
        pairs(&second),
        5,
        "c against a and b, then d against a, b and c"
    );
    assert_eq!(
        comparisons(&second),
        4 * 5,
        "each pair compared exactly once: 5 pairs of 2x2 chunks"
    );
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
    let unique: BTreeSet<_> = fixture
        .rows(&fixture.space)
        .into_iter()
        .map(|row| (row.0, row.1, row.2))
        .collect();
    assert_eq!(
        unique.len(),
        fixture.rows(&fixture.space).len(),
        "no pair produced duplicate candidates"
    );
}

#[test]
fn stopping_mid_pair_and_resuming_equals_an_uninterrupted_run() {
    let build = || {
        let mut fixture = Fixture::new();
        fixture.put("a", &chunks(7, "a", 0.00), true);
        fixture.put("b", &chunks(6, "b", 0.02), true);
        fixture.put("c", &chunks(5, "c", 0.04), true);
        fixture
    };
    let mut straight = build();
    straight.run_to_complete(DiscoveryLimits::default());

    let mut interrupted = build();
    let cancel = AtomicBool::new(false);
    let one_tile_per_turn = DiscoveryLimits {
        max_run_work: 10_000,
        ..small_tiles()
    };
    let summary =
        interrupted.run_with(one_tile_per_turn, &cancel, &|_| Ok(true), &mut |progress| {
            if progress.tiles == 3 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
    assert_eq!(summary.end, RunEnd::Cancelled);
    assert_eq!(
        summary.progress.tiles, 3,
        "stopped at a tile boundary, completed tiles kept"
    );
    let saved = interrupted.progress_rows();
    assert!(
        !saved.is_empty() && saved.iter().any(|row| row.2 > 0 || row.3 > 0),
        "the in-flight pair's cursor survives the stop: {saved:?}"
    );
    assert!(interrupted.coverage().pairs_remaining > 0);

    interrupted.run_to_complete(small_tiles());
    assert_eq!(interrupted.coverage().state, CoverageState::Complete);
    assert_eq!(
        interrupted.rows(&interrupted.space),
        straight.rows(&straight.space),
        "same candidates, same evidence, same ranking"
    );
}

#[test]
fn a_pair_larger_than_a_run_completes_over_several_runs_each_advancing_a_tile() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(6, "a", 0.00), true);
    fixture.put("b", &chunks(6, "b", 0.02), true);
    // 36 cells in 2x2 tiles is 9 tiles; a one-comparison budget is raised to one tile.
    let limits = DiscoveryLimits {
        tile_rows: 2,
        tile_cols: 2,
        max_run_work: 1,
        tiles_per_job_per_turn: 4,
    };
    let runs = fixture.run_to_complete(limits);
    let working: Vec<_> = runs.iter().filter(|run| run.progress.tiles > 0).collect();
    assert_eq!(working.len(), 9);
    assert!(
        working.iter().all(|run| run.progress.tiles == 1),
        "a run is never smaller than one tile and never larger than its budget"
    );
    assert!(working[..8]
        .iter()
        .all(|run| run.end == RunEnd::BudgetExhausted));
    assert_eq!(
        working[8].end,
        RunEnd::Complete,
        "the ninth tile finishes the pair and the run finds nothing left"
    );
    assert_eq!(comparisons(&runs), 36);
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
}

#[test]
fn round_robin_serves_every_runnable_job_across_runs() {
    let mut fixture = Fixture::new();
    for (index, name) in ["a", "b", "c", "d"].iter().enumerate() {
        fixture.put(name, &chunks(8, name, index as f32 * 0.01), true);
    }
    // A tiny, one-tile run: jobs b, c and d are runnable.
    for _ in 0..3 {
        let summary = fixture.run(small_tiles());
        assert_eq!(summary.progress.tiles, 1);
    }
    let served: BTreeSet<String> = fixture
        .progress_rows()
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(
        served,
        BTreeSet::from(["b".to_owned(), "c".to_owned(), "d".to_owned()]),
        "each job advanced once before any got a second turn"
    );

    // A late, tiny document finishes while the big jobs are still going.
    fixture.put("e", &chunks(1, "e", 0.05), true);
    let mut runs = 0;
    while fixture.conn.query_row::<i64, _, _>(
        "SELECT count(*) FROM ai_relationship_coverage r WHERE r.document_id = 'e' AND r.partner_cursor_seq < (SELECT max(p.seq) FROM ai_relationship_coverage p WHERE p.seq < r.seq)",
        [],
        |row| row.get(0),
    ).unwrap() > 0 {
        fixture.run(small_tiles());
        runs += 1;
        assert!(runs <= 4 * 5, "the small job is not starved by the big ones");
    }
    assert!(
        fixture.coverage().pairs_remaining > 0,
        "the large jobs are not done yet"
    );
}

#[test]
fn an_edit_between_compute_and_commit_drops_that_tile_and_its_progress() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(6, "a", 0.00), true);
    fixture.put("b", &chunks(6, "b", 0.02), true);
    let first = fixture.run(small_tiles());
    assert_eq!(first.progress.tiles, 1);
    assert_eq!(fixture.progress_rows().len(), 1);

    let edited = std::cell::Cell::new(false);
    let rewritten = chunks(3, "a-rewritten", 0.5);
    let versions = &mut fixture.versions;
    versions.insert("a".into(), 9);
    let summary = {
        let new_hash = "sha256:a:9".to_owned();
        let rewritten = &rewritten;
        let edited = &edited;
        let space = fixture.space.clone();
        let still_active = move |conn: &Connection| -> crate::db::NativeResult<bool> {
            if !edited.replace(true) {
                // The racing writer: Local Sync replaces a's content between the
                // tile's computation and its commit.
                let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).unwrap();
                index::clear_derived(&tx, "a").unwrap();
                tx.execute(
                    "UPDATE documents SET content_hash = ?2 WHERE id = ?1",
                    params!["a", new_hash],
                )
                .unwrap();
                for (ordinal, (text, vector)) in rewritten.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO chunks (document_id, ordinal, chunk_text, start_offset, end_offset, page, content_hash) VALUES ('a', ?1, ?2, 0, ?3, NULL, ?4)",
                        params![ordinal as i64, text, text.len() as i64, format!("c:{text}")],
                    )
                    .unwrap();
                    embed(&tx, tx.last_insert_rowid(), &space, vector);
                }
                tx.commit().unwrap();
            }
            Ok(true)
        };
        fixture.run_with(
            small_tiles(),
            &AtomicBool::new(false),
            &still_active,
            &mut |_| {},
        )
    };
    assert!(edited.get());
    // The aborted tile committed nothing; the stale pair progress is gone.
    assert_eq!(summary.progress.tiles, 0);
    assert!(
        fixture
            .progress_rows()
            .iter()
            .all(|row| row.1 != "a" && row.0 != "a"),
        "no progress names the replaced revision"
    );
    assert!(
        fixture.conn.query_row::<i64, _, _>("SELECT count(*) FROM ai_relationship_coverage WHERE document_id = 'a' AND content_hash = 'sha256:a:1'", [], |row| row.get(0)).unwrap() == 0,
        "the old revision's coverage is gone"
    );
    // The new revision is admitted afresh and everything completes against it.
    fixture.run_to_complete(small_tiles());
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
    for (_, _, _, evidence, _) in fixture.rows(&fixture.space) {
        assert!(
            !evidence.contains("a paragraph"),
            "no evidence from the replaced revision survives: {evidence}"
        );
    }
}

#[test]
fn a_space_change_stops_the_run_before_committing_any_tile() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(3, "a", 0.00), true);
    fixture.put("b", &chunks(3, "b", 0.02), true);
    let summary = fixture.run_with(
        DiscoveryLimits::default(),
        &AtomicBool::new(false),
        &|_| Ok(false),
        &mut |_| {},
    );
    assert_eq!(summary.end, RunEnd::SpaceChanged);
    assert_eq!(summary.progress.tiles, 0);
    assert!(fixture.progress_rows().is_empty());
    assert!(fixture.rows(&fixture.space).is_empty());
    assert_eq!(fixture.coverage().state, CoverageState::Partial);
}

#[test]
fn discovery_reads_only_the_active_space_even_when_dimensions_match() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(2, "a", 0.00), true);
    fixture.put("b", &chunks(2, "b", 0.02), true);
    // A second space of the same dimension holds orthogonal vectors for both.
    let other = register(&fixture.conn, "r2");
    for (name, vector) in [("a", [1.0f32, 0.0]), ("b", [0.0f32, 1.0])] {
        let ids: Vec<i64> = fixture
            .conn
            .prepare("SELECT chunk_id FROM chunks WHERE document_id = ?1")
            .unwrap()
            .query_map([name], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let tx = fixture.conn.transaction().unwrap();
        for id in ids {
            embed(&tx, id, &other, &vector);
        }
        tx.commit().unwrap();
    }
    fixture.run_to_complete(DiscoveryLimits::default());
    assert!(
        !fixture.rows(&fixture.space).is_empty(),
        "the active space's similar vectors produce a candidate"
    );
    assert!(
        fixture.rows(&other).is_empty(),
        "nothing is stored for, or read from, the other space"
    );
    assert_eq!(
        coverage(&fixture.conn, WORKSPACE, Some(&other))
            .unwrap()
            .state,
        CoverageState::Partial
    );
}

#[test]
fn coverage_states_are_honest() {
    let mut fixture = Fixture::new();
    assert_eq!(
        coverage(&fixture.conn, WORKSPACE, None).unwrap().state,
        CoverageState::NoActiveSpace
    );
    assert_eq!(
        fixture.coverage().state,
        CoverageState::Complete,
        "an empty folder has nothing left to compare"
    );

    fixture.put("a", &chunks(2, "a", 0.00), true);
    fixture.put("b", &chunks(2, "b", 0.02), false);
    assert_eq!(fixture.coverage().state, CoverageState::EmbeddingIncomplete);
    fixture.embed_pending("b", &chunks(2, "b", 0.02));
    assert_eq!(fixture.coverage().state, CoverageState::Partial);
    fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
}

#[test]
fn byte_identical_documents_are_skipped_but_counted_as_compared() {
    let mut fixture = Fixture::new();
    let same = chunks(2, "same", 0.0);
    fixture.put("a", &same, true);
    fixture.put("b", &same, true);
    // Force the same revision hash, as duplicate files have.
    fixture
        .conn
        .execute("UPDATE documents SET content_hash = 'sha256:same'", [])
        .unwrap();
    fixture
        .conn
        .execute("DELETE FROM ai_relationship_coverage", [])
        .unwrap();
    let runs = fixture.run_to_complete(DiscoveryLimits::default());
    assert_eq!(comparisons(&runs), 0);
    assert!(
        fixture.rows(&fixture.space).is_empty(),
        "duplicates are left to duplicate detection"
    );
    assert_eq!(fixture.coverage().state, CoverageState::Complete);
}

#[test]
fn multi_chunk_documents_with_document_level_offsets_are_read_correctly() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(9, "a", 0.00), true);
    fixture.put("b", &chunks(9, "b", 0.01), true);
    fixture.run_to_complete(DiscoveryLimits::default());
    let rows = fixture.rows(&fixture.space);
    assert!(!rows.is_empty());
    // Evidence offsets are the stored document-level offsets, not chunk-relative.
    assert!(
        rows.iter().any(|row| row.3.contains("\"start\":")),
        "{rows:?}"
    );
    let stored = index::list_relationships(&fixture.conn, WORKSPACE, Some(&fixture.space)).unwrap();
    assert!(!stored.is_empty());
}

// ------------------------------------------------- the #27 integration gate

mod integration_with_embedding_sync {
    use super::*;
    use std::fs;

    use folio_core::contracts::{ModelDescriptor, ModelFile, ModelRole};
    use folio_core::embeddings::{e5_inputs_from_descriptor, e5_provider_space, E5_DIMENSIONS};

    use crate::active_space::{persistent_space_for_descriptor, resolve_installed_descriptor};
    use crate::embedding_sync::{
        stored_index_space, stored_space_fingerprint, sync_embeddings, IndexChunkStore,
        PassageEmbedder, SyncLimits,
    };
    use crate::error::FolioError;
    use crate::index::list_relationships;
    use crate::index::tests::{authorize, scan};

    fn model_descriptor(revision: &str) -> ModelDescriptor {
        ModelDescriptor {
            id: "multilingual-e5-small".into(),
            role: ModelRole::Embedding,
            repo: "example/e5".into(),
            revision: revision.into(),
            files: vec![
                ModelFile {
                    path: "onnx/model_quantized.onnx".into(),
                    sha256: "sha256:model".into(),
                    bytes: 1,
                    download_url: None,
                },
                ModelFile {
                    path: "tokenizer.json".into(),
                    sha256: "sha256:tokenizer".into(),
                    bytes: 1,
                    download_url: None,
                },
            ],
            quantization: "int8".into(),
            license: "mit".into(),
            runtime: "onnx".into(),
            optional_pack: false,
        }
    }

    /// A deterministic stand-in for the provider: hashed bag of words, so
    /// documents sharing words are near each other.
    struct BagOfWords(folio_core::contracts::EmbeddingSpace);

    impl PassageEmbedder for BagOfWords {
        fn embed_batch(
            &mut self,
            texts: &[String],
            _cancel: &AtomicBool,
        ) -> Result<(folio_core::contracts::EmbeddingSpace, Vec<Vec<f32>>), FolioError> {
            let vectors = texts
                .iter()
                .map(|text| {
                    let mut vector = vec![0.0f32; E5_DIMENSIONS];
                    for word in text
                        .to_lowercase()
                        .split(|c: char| !c.is_alphanumeric())
                        .filter(|w| w.len() > 2)
                    {
                        let bucket = word.bytes().fold(7usize, |acc, byte| {
                            acc.wrapping_mul(31).wrapping_add(byte as usize)
                        }) % E5_DIMENSIONS;
                        vector[bucket] += 1.0;
                    }
                    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt().max(1.0);
                    vector.iter().map(|v| v / norm).collect()
                })
                .collect();
            Ok((self.0.clone(), vectors))
        }
    }

    #[test]
    fn real_embedding_sync_registers_exactly_the_space_the_resolver_and_discovery_read() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("plan.md"), "The community learning project submission deadline is October twenty for the whole team.").unwrap();
        fs::write(folder.path().join("notes.md"), "Reminder: the community learning project submission deadline is October twenty for every team.").unwrap();
        fs::write(
            folder.path().join("recipe.md"),
            "Boil the noodles with garlic, soy sauce and a little pepper before serving.",
        )
        .unwrap();
        // A file-backed index: the embedding store keeps its own connection, as
        // #27's native sync does.
        let database = tempfile::tempdir().unwrap();
        let path = database.path().join("folio.sqlite");
        let mut conn = db::open(&path).unwrap();
        let root = authorize(&conn, folder.path());
        scan(&mut conn, &root);

        let descriptor = model_descriptor("rev-1");
        let provider_space =
            e5_provider_space(&e5_inputs_from_descriptor(&descriptor).unwrap().inputs);
        let stored = stored_index_space(&provider_space).unwrap();

        // Nothing is registered yet: no active space.
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&descriptor)).unwrap(),
            None
        );

        // #27's sequence: register the stored space, then fill it from pending chunks.
        let fingerprint = index::register_space(&conn, &stored).unwrap();
        assert_eq!(
            fingerprint,
            stored_space_fingerprint(&provider_space).unwrap()
        );
        let before = coverage(&conn, &root.id, Some(&fingerprint)).unwrap();
        assert_eq!(
            (
                before.state,
                before.indexed_documents,
                before.eligible_documents
            ),
            (CoverageState::EmbeddingIncomplete, 3, 0)
        );

        let mut store = IndexChunkStore::new(
            db::open(&path).unwrap(),
            root.id.clone(),
            fingerprint.clone(),
        );
        let mut embedder = BagOfWords(provider_space.clone());
        let summary = sync_embeddings(
            &mut store,
            &mut embedder,
            &provider_space,
            &fingerprint,
            root.id.clone(),
            &AtomicBool::new(false),
            SyncLimits::default(),
        )
        .unwrap();
        assert!(summary.complete);

        // The resolver returns exactly #27's registered space.
        let active = resolve_installed_descriptor(&conn, Some(&descriptor)).unwrap();
        assert_eq!(active.as_deref(), Some(fingerprint.as_str()));
        assert_eq!(
            persistent_space_for_descriptor(&descriptor)
                .unwrap()
                .unwrap()
                .dimensions as usize,
            E5_DIMENSIONS
        );

        // Discovery reads only that space and proves completeness.
        let cancel = AtomicBool::new(false);
        let model = descriptor.clone();
        let still_active = move |conn: &Connection| {
            Ok(resolve_installed_descriptor(conn, Some(&model))?.is_some())
        };
        let run = run_discovery(
            &mut conn,
            &RunContext {
                workspace_id: &root.id,
                space: &fingerprint,
                limits: DiscoveryLimits::default(),
                cancel: &cancel,
                still_active: &still_active,
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(run.end, RunEnd::Complete);
        let covered = coverage(&conn, &root.id, Some(&fingerprint)).unwrap();
        assert_eq!(
            (
                covered.state,
                covered.eligible_documents,
                covered.pairs_remaining
            ),
            (CoverageState::Complete, 3, 0)
        );
        let listed = list_relationships(&conn, &root.id, active.as_deref()).unwrap();
        let (plan, notes) = (
            crate::identity::document_id(&root.id, "plan.md"),
            crate::identity::document_id(&root.id, "notes.md"),
        );
        assert!(listed.iter().any(|relationship| matches!(relationship, index::Relationship::Similarity(edge) if (edge.source_id == plan && edge.target_id == notes) || (edge.source_id == notes && edge.target_id == plan))), "the two deadline notes are connected");
        assert!(listed.iter().all(|relationship| match relationship {
            index::Relationship::Similarity(edge) => edge.space_fingerprint == fingerprint,
            _ => true,
        }));

        // A model change invalidates display and stops persistence.
        let changed = model_descriptor("rev-2");
        assert_eq!(
            resolve_installed_descriptor(&conn, Some(&changed)).unwrap(),
            None
        );
        fs::write(
            folder.path().join("extra.md"),
            "Another community learning project deadline note for the team.",
        )
        .unwrap();
        scan(&mut conn, &root);
        let embedded = sync_embeddings(
            &mut store,
            &mut embedder,
            &provider_space,
            &fingerprint,
            root.id.clone(),
            &AtomicBool::new(false),
            SyncLimits::default(),
        )
        .unwrap();
        assert!(embedded.complete);
        let changed_for_run = changed.clone();
        let stale_model = move |conn: &Connection| {
            Ok(resolve_installed_descriptor(conn, Some(&changed_for_run))?.is_some())
        };
        let stopped = run_discovery(
            &mut conn,
            &RunContext {
                workspace_id: &root.id,
                space: &fingerprint,
                limits: DiscoveryLimits::default(),
                cancel: &cancel,
                still_active: &stale_model,
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(
            (stopped.end, stopped.progress.tiles),
            (RunEnd::SpaceChanged, 0)
        );
    }
}

#[test]
fn shared_fact_candidates_are_stored_only_for_a_corroborated_fact() {
    let mut fixture = Fixture::new();
    let vector = [1.0f32, 0.0];
    let one = |text: &str| vec![(text.to_owned(), vector)];
    fixture.put(
        "a-plan",
        &one("# Community Learning Project\n\nThe project submission deadline is October 20."),
        true,
    );
    fixture.put(
        "b-tala",
        &one("Ang huling araw ng pagpasa ng Community Learning Project ay October 20."),
        true,
    );
    fixture.put("c-math", &one("The mathematics practice session is October 20; this is a different event from the Community Learning Project deadline."), true);
    fixture.run_to_complete(DiscoveryLimits::default());
    let shared: BTreeSet<(String, String)> = fixture
        .rows(&fixture.space)
        .into_iter()
        .filter(|row| row.2 == "sharedFactCandidate")
        .map(|row| (row.0, row.1))
        .collect();
    assert_eq!(
        shared,
        BTreeSet::from([("a-plan".to_owned(), "b-tala".to_owned())]),
        "the unrelated event on the same date is not a shared fact"
    );
    let similar = fixture
        .rows(&fixture.space)
        .into_iter()
        .filter(|row| row.2 == "similarity")
        .count();
    assert_eq!(
        similar, 3,
        "identical vectors still make all three documents similar"
    );
}

#[test]
fn starting_in_a_new_space_purges_the_old_spaces_rows_but_keeps_links_and_vectors() {
    let mut fixture = Fixture::new();
    fixture.put("a", &chunks(2, "a", 0.00), true);
    fixture.put("b", &chunks(2, "b", 0.02), true);
    fixture.run_to_complete(DiscoveryLimits::default());
    let old_space = fixture.space.clone();
    assert!(!fixture.rows(&old_space).is_empty());
    fixture
        .conn
        .execute(
            "INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES ('link', 'a', 'b', 'explicitReference', '{}', 'documentLink', NULL, 'h', 'h', '0')",
            [],
        )
        .unwrap();
    let new_space = register(&fixture.conn, "r2");
    crate::ai_discovery::purge_other_spaces(&mut fixture.conn, WORKSPACE, &new_space).unwrap();
    assert!(fixture.rows(&old_space).is_empty());
    let count = |table: &str| -> i64 {
        fixture
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    assert_eq!(count("ai_relationship_coverage"), 0);
    assert_eq!(count("ai_relationship_seq"), 0);
    assert_eq!(
        count("relationships WHERE relationship_type = 'explicitReference'"),
        1,
        "links are never purged"
    );
    assert!(
        count("embeddings") > 0,
        "vectors belong to the embedding store"
    );
}
