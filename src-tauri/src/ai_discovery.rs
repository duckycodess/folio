//! Progressive, resumable AI relationship discovery over one persistent
//! embedding space (#46).
//!
//! Work is admitted, ordered and bounded so that it can stop at any tile
//! boundary and resume later without losing or repeating completed work:
//!
//! - **Admission.** An *eligible* revision (an indexed document whose every
//!   chunk has a vector in the space) receives `seq` from a per-(workspace,
//!   space) counter that only ever grows, so a seq is never reused.
//! - **Ownership.** For admitted X and Y with `X.seq < Y.seq`, Y's job
//!   compares the pair against X's current revision. Each pair has one owner.
//! - **Tiles.** A pair is compared in bounded tiles of chunks. The only
//!   thing kept between tiles is a small persisted accumulator and a cursor.
//!   Candidate edges are written only when a pair's last tile is done.
//! - **Fairness.** Jobs are served round-robin by seq with a persisted
//!   pointer and a few tiles per turn, so no document takes a whole run.
//! - **Commits.** Every tile commits in one `Immediate` transaction that
//!   re-checks document hashes and coverage. No lock is held while comparing.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use folio_core::relationships::{
    finish_pair, process_tile, referenced_positions, tile_cost, PairAccumulator, PairSide,
    RelationshipChunk, MAX_RUN_COMPARISONS, MAX_TILE_COMPARISONS, TILES_PER_JOB_PER_TURN,
    TILE_COLS, TILE_ROWS,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;

use crate::db::NativeResult;
use crate::error::{error, ErrorCode};
use crate::index::{self, check_vector, space_dimensions};

/// Chunks that make an indexed document *eligible* in a space: it has at least
/// one chunk and every chunk has a vector there. Binds `?1` workspace and `?2`
/// space.
const ELIGIBLE_DOCUMENT: &str = "d.workspace_id = ?1 AND d.status = 'indexed' AND d.content_hash != '' \
     AND EXISTS (SELECT 1 FROM chunks c WHERE c.document_id = d.id) \
     AND NOT EXISTS (SELECT 1 FROM chunks c WHERE c.document_id = d.id AND NOT EXISTS (SELECT 1 FROM embeddings e WHERE e.chunk_id = c.chunk_id AND e.space_id = ?2))";

#[derive(Clone, Copy, Debug)]
pub struct DiscoveryLimits {
    pub tile_rows: usize,
    pub tile_cols: usize,
    pub max_run_work: usize,
    pub tiles_per_job_per_turn: usize,
}

impl Default for DiscoveryLimits {
    fn default() -> Self {
        Self {
            tile_rows: TILE_ROWS,
            tile_cols: TILE_COLS,
            max_run_work: MAX_RUN_COMPARISONS,
            tiles_per_job_per_turn: TILES_PER_JOB_PER_TURN,
        }
    }
}

impl DiscoveryLimits {
    /// The effective limits: a tile never exceeds `MAX_TILE_COMPARISONS`, and
    /// a run is never smaller than one tile, so every run makes progress.
    fn effective(self) -> Self {
        let tile_rows = self.tile_rows.clamp(1, TILE_ROWS);
        let tile_cols = self.tile_cols.clamp(1, TILE_COLS);
        Self {
            tile_rows,
            tile_cols,
            max_run_work: self
                .max_run_work
                .max(tile_cost(tile_rows, tile_cols))
                .max(1),
            tiles_per_job_per_turn: self.tiles_per_job_per_turn.max(1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunEnd {
    /// Nothing admitted has work left.
    Complete,
    /// The run's comparison budget is spent; a later run resumes.
    BudgetExhausted,
    /// Stopped at a tile boundary; completed tiles stay.
    Cancelled,
    /// The selected model or its space changed; no further tile was committed.
    SpaceChanged,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryProgress {
    pub admitted: usize,
    pub tiles: usize,
    pub comparisons: usize,
    /// Comparisons plus clause-feature work: what the run budget counts.
    pub work: usize,
    pub pairs_completed: usize,
    pub edges_stored: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub progress: DiscoveryProgress,
    pub end: RunEnd,
}

pub struct RunContext<'a> {
    pub workspace_id: &'a str,
    pub space: &'a str,
    pub limits: DiscoveryLimits,
    pub cancel: &'a AtomicBool,
    /// Re-resolves the active space (outside any lock) before every commit. A
    /// `false` stops the run without committing that tile.
    pub still_active: &'a dyn Fn(&Connection) -> NativeResult<bool>,
}

#[derive(Clone, Debug)]
struct Job {
    document_id: String,
    content_hash: String,
    seq: i64,
}

/// Admits eligible revisions that have no coverage row, in document-id order,
/// and drops coverage whose revision no longer matches the document. Returns
/// how many revisions were admitted.
pub fn admit_eligible(
    conn: &mut Connection,
    workspace_id: &str,
    space: &str,
    now: u64,
) -> NativeResult<usize> {
    space_dimensions(conn, space)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT OR IGNORE INTO ai_relationship_seq (workspace_id, space_id, next_seq) VALUES (?1, ?2, 1)",
        params![workspace_id, space],
    )?;
    let stale: Vec<String> = tx
        .prepare("SELECT r.document_id FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id WHERE r.workspace_id = ?1 AND r.space_id = ?2 AND r.content_hash != d.content_hash")?
        .query_map(params![workspace_id, space], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for document_id in stale {
        reset_document(&tx, workspace_id, space, &document_id)?;
    }
    let new: Vec<(String, String)> = tx
        .prepare(&format!(
            "SELECT d.id, d.content_hash FROM documents d WHERE {ELIGIBLE_DOCUMENT} AND NOT EXISTS (SELECT 1 FROM ai_relationship_coverage r WHERE r.workspace_id = d.workspace_id AND r.space_id = ?2 AND r.document_id = d.id) ORDER BY d.id"
        ))?
        .query_map(params![workspace_id, space], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if new.is_empty() {
        tx.commit()?;
        return Ok(0);
    }
    let mut next: i64 = tx.query_row(
        "SELECT next_seq FROM ai_relationship_seq WHERE workspace_id = ?1 AND space_id = ?2",
        params![workspace_id, space],
        |row| row.get(0),
    )?;
    for (document_id, content_hash) in &new {
        tx.execute(
            "INSERT INTO ai_relationship_coverage (workspace_id, space_id, document_id, content_hash, seq, partner_cursor_seq, candidate_overflow, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6)",
            params![workspace_id, space, document_id, content_hash, next, now.to_string()],
        )?;
        next += 1;
    }
    tx.execute(
        "UPDATE ai_relationship_seq SET next_seq = ?3 WHERE workspace_id = ?1 AND space_id = ?2",
        params![workspace_id, space, next],
    )?;
    tx.commit()?;
    Ok(new.len())
}

/// Forgets one document's discovery state in one space. The counter is not
/// touched, so the revision is re-admitted with a larger seq.
fn reset_document(
    tx: &Transaction<'_>,
    workspace_id: &str,
    space: &str,
    document_id: &str,
) -> NativeResult<()> {
    tx.execute(
        "DELETE FROM relationships WHERE space_fingerprint = ?1 AND relationship_type IN ('similarity', 'sharedFactCandidate') AND (source_document_id = ?2 OR target_document_id = ?2)",
        params![space, document_id],
    )?;
    tx.execute(
        "DELETE FROM ai_pair_progress WHERE workspace_id = ?1 AND space_id = ?2 AND (document_id = ?3 OR partner_id = ?3)",
        params![workspace_id, space, document_id],
    )?;
    tx.execute(
        "DELETE FROM ai_relationship_coverage WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
        params![workspace_id, space, document_id],
    )?;
    Ok(())
}

fn schedule_pointer(conn: &Connection, workspace_id: &str, space: &str) -> NativeResult<i64> {
    Ok(conn
        .query_row(
            "SELECT next_job_seq FROM ai_relationship_schedule WHERE workspace_id = ?1 AND space_id = ?2",
            params![workspace_id, space],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn set_schedule_pointer(
    conn: &Connection,
    workspace_id: &str,
    space: &str,
    pointer: i64,
) -> NativeResult<()> {
    conn.execute(
        "INSERT INTO ai_relationship_schedule (workspace_id, space_id, next_job_seq) VALUES (?1, ?2, ?3) ON CONFLICT(workspace_id, space_id) DO UPDATE SET next_job_seq = excluded.next_job_seq",
        params![workspace_id, space, pointer],
    )?;
    Ok(())
}

/// The next runnable job at or after `from_seq`, wrapping to the start.
fn next_job(
    conn: &Connection,
    workspace_id: &str,
    space: &str,
    from_seq: i64,
    skipped: &HashSet<String>,
) -> NativeResult<Option<Job>> {
    let runnable = |from: i64| -> NativeResult<Option<Job>> {
        let mut statement = conn.prepare(
            "SELECT r.document_id, r.content_hash, r.seq FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id AND d.content_hash = r.content_hash \
             WHERE r.workspace_id = ?1 AND r.space_id = ?2 AND r.seq >= ?3 \
               AND (EXISTS (SELECT 1 FROM ai_pair_progress g WHERE g.workspace_id = r.workspace_id AND g.space_id = r.space_id AND g.document_id = r.document_id) \
                 OR EXISTS (SELECT 1 FROM ai_relationship_coverage p WHERE p.workspace_id = r.workspace_id AND p.space_id = r.space_id AND p.seq > r.partner_cursor_seq AND p.seq < r.seq)) \
             ORDER BY r.seq LIMIT 64",
        )?;
        let rows = statement.query_map(params![workspace_id, space, from], |row| {
            Ok(Job {
                document_id: row.get(0)?,
                content_hash: row.get(1)?,
                seq: row.get(2)?,
            })
        })?;
        for row in rows {
            let job = row?;
            if !skipped.contains(&job.document_id) {
                return Ok(Some(job));
            }
        }
        Ok(None)
    };
    match runnable(from_seq)? {
        Some(job) => Ok(Some(job)),
        None if from_seq > 0 => runnable(0),
        None => Ok(None),
    }
}

struct PairState {
    partner_id: String,
    document_hash: String,
    partner_hash: String,
    partner_seq: i64,
    next_left: usize,
    next_right: usize,
    accumulator: PairAccumulator,
    /// Whether this state is a stored progress row (as opposed to a fresh pair).
    stored: bool,
}

enum Step {
    Tile {
        comparisons: usize,
        work: usize,
        pair_done: bool,
        edges: usize,
    },
    NoPartner,
    Aborted,
    OutOfBudget,
    Cancelled,
    SpaceChanged,
}

/// Admits eligible revisions, then serves jobs fairly until nothing is left,
/// the comparison budget is spent, a Stop arrives or the space changes. Every
/// committed tile survives any of those.
pub fn run_discovery(
    conn: &mut Connection,
    context: &RunContext<'_>,
    progress: &mut dyn FnMut(&DiscoveryProgress),
) -> NativeResult<RunSummary> {
    let limits = context.limits.effective();
    let mut summary = DiscoveryProgress::default();
    summary.admitted = admit_eligible(conn, context.workspace_id, context.space, index::now_ms())?;
    progress(&summary);
    let mut pointer = schedule_pointer(conn, context.workspace_id, context.space)?;
    let mut skipped = HashSet::new();
    let end = 'run: loop {
        if context.cancel.load(Ordering::SeqCst) {
            break RunEnd::Cancelled;
        }
        let Some(job) = next_job(conn, context.workspace_id, context.space, pointer, &skipped)?
        else {
            break RunEnd::Complete;
        };
        for _ in 0..limits.tiles_per_job_per_turn {
            let remaining = limits.max_run_work.saturating_sub(summary.work);
            match step(conn, context, &limits, &job, remaining, summary.tiles == 0)? {
                Step::Tile {
                    comparisons,
                    work,
                    pair_done,
                    edges,
                } => {
                    summary.tiles += 1;
                    summary.comparisons += comparisons;
                    summary.work += work;
                    summary.edges_stored += edges;
                    summary.pairs_completed += usize::from(pair_done);
                    progress(&summary);
                }
                Step::NoPartner => break,
                Step::Aborted => {
                    skipped.insert(job.document_id.clone());
                    break;
                }
                Step::OutOfBudget => break 'run RunEnd::BudgetExhausted,
                Step::Cancelled => break 'run RunEnd::Cancelled,
                Step::SpaceChanged => break 'run RunEnd::SpaceChanged,
            }
        }
        pointer = job.seq + 1;
        set_schedule_pointer(conn, context.workspace_id, context.space, pointer)?;
    };
    Ok(RunSummary {
        progress: summary,
        end,
    })
}

fn chunk_count(conn: &Connection, document_id: &str) -> NativeResult<usize> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM chunks WHERE document_id = ?1",
        [document_id],
        |row| row.get(0),
    )?;
    Ok(count as usize)
}

/// One bounded step of one job: the next tile of its current pair (starting
/// the next partner when none is in flight).
fn step(
    conn: &mut Connection,
    context: &RunContext<'_>,
    limits: &DiscoveryLimits,
    job: &Job,
    remaining: usize,
    first_tile: bool,
) -> NativeResult<Step> {
    let workspace = context.workspace_id;
    let space = context.space;
    // The job struct was read when the turn started; earlier tiles of the
    // same turn may have finished pairs since, so read the cursor afresh.
    let cursor: i64 = conn.query_row(
        "SELECT partner_cursor_seq FROM ai_relationship_coverage WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
        params![workspace, space, job.document_id],
        |row| row.get(0),
    )?;
    let stored: Option<PairState> = conn
        .query_row(
            "SELECT partner_id, document_hash, partner_hash, partner_seq, next_left, next_right, accumulator_json FROM ai_pair_progress WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
            params![workspace, space, job.document_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?
        .map(|(partner_id, document_hash, partner_hash, partner_seq, left, right, json)| {
            Ok::<_, crate::error::FolioError>(PairState {
                partner_id,
                document_hash,
                partner_hash,
                partner_seq,
                next_left: left as usize,
                next_right: right as usize,
                accumulator: serde_json::from_str(&json)?,
                stored: true,
            })
        })
        .transpose()?;
    let state = match stored {
        Some(state) => state,
        None => {
            let partner: Option<(String, String, i64)> = conn
                .query_row(
                    "SELECT r.document_id, r.content_hash, r.seq FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id AND d.content_hash = r.content_hash \
                     WHERE r.workspace_id = ?1 AND r.space_id = ?2 AND r.seq > ?3 AND r.seq < ?4 ORDER BY r.seq LIMIT 1",
                    params![workspace, space, cursor, job.seq],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let Some((partner_id, partner_hash, partner_seq)) = partner else {
                return Ok(Step::NoPartner);
            };
            PairState {
                partner_id,
                document_hash: job.content_hash.clone(),
                partner_hash,
                partner_seq,
                next_left: 0,
                next_right: 0,
                accumulator: PairAccumulator::default(),
                stored: false,
            }
        }
    };
    if state.document_hash != job.content_hash {
        return Ok(Step::Aborted);
    }

    let left_total = chunk_count(conn, &job.document_id)?;
    let right_total = chunk_count(conn, &state.partner_id)?;
    // Byte-identical documents are left to duplicate detection, and a
    // document without chunks has nothing to compare: the pair is done.
    let trivial = state.document_hash == state.partner_hash || left_total == 0 || right_total == 0;
    let (rows, columns) = if trivial || state.next_left >= left_total {
        (0, 0)
    } else {
        (
            limits.tile_rows.min(left_total - state.next_left),
            limits
                .tile_cols
                .min(right_total.saturating_sub(state.next_right)),
        )
    };
    let cost = rows * columns;
    let work = if cost == 0 {
        0
    } else {
        tile_cost(rows, columns)
    };
    debug_assert!(work <= MAX_TILE_COMPARISONS);
    if !first_tile && work > remaining {
        return Ok(Step::OutOfBudget);
    }
    if context.cancel.load(Ordering::SeqCst) {
        return Ok(Step::Cancelled);
    }

    let mut accumulator = state.accumulator.clone();
    let (next_left, next_right, pair_done) = if cost == 0 {
        (state.next_left, state.next_right, true)
    } else {
        let dimensions = space_dimensions(conn, space)?;
        let Some(left) = load_chunks(
            conn,
            &job.document_id,
            &job.content_hash,
            state.next_left,
            rows,
            space,
            dimensions,
        )?
        else {
            return Ok(Step::Aborted);
        };
        let Some(right) = load_chunks(
            conn,
            &state.partner_id,
            &state.partner_hash,
            state.next_right,
            columns,
            space,
            dimensions,
        )?
        else {
            return Ok(Step::Aborted);
        };
        match process_tile(
            &mut accumulator,
            &left,
            state.next_left,
            &right,
            state.next_right,
            Some(context.cancel),
        ) {
            Ok(_) => {}
            Err(failure) if failure.to_string().contains("cancelled") => {
                return Ok(Step::Cancelled)
            }
            Err(failure) => return Err(crate::ai_boundary::core_failure(failure)),
        }
        let mut next_right = state.next_right + columns;
        let mut next_left = state.next_left;
        if next_right >= right_total {
            next_right = 0;
            next_left += rows;
        }
        (next_left, next_right, next_left >= left_total)
    };

    // Re-resolve the active space outside any lock; a change stops the run
    // before this tile is committed.
    if !(context.still_active)(conn)? {
        return Ok(Step::SpaceChanged);
    }

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !tile_is_current(&tx, workspace, space, job, &state, cursor)? {
        return Ok(Step::Aborted);
    }
    let mut edges_stored = 0;
    if !pair_done {
        tx.execute(
            "INSERT INTO ai_pair_progress (workspace_id, space_id, document_id, partner_id, document_hash, partner_hash, partner_seq, next_left, next_right, accumulator_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(workspace_id, space_id, document_id) DO UPDATE SET next_left = excluded.next_left, next_right = excluded.next_right, accumulator_json = excluded.accumulator_json",
            params![
                workspace,
                space,
                job.document_id,
                state.partner_id,
                state.document_hash,
                state.partner_hash,
                state.partner_seq,
                next_left as i64,
                next_right as i64,
                serde_json::to_string(&accumulator)?,
            ],
        )?;
    } else {
        if !trivial {
            let (left_positions, right_positions) = referenced_positions(&accumulator);
            let left = PairSide {
                id: job.document_id.clone(),
                content_hash: job.content_hash.clone(),
                chunks: load_positions(&tx, &job.document_id, &job.content_hash, &left_positions)?,
            };
            let right = PairSide {
                id: state.partner_id.clone(),
                content_hash: state.partner_hash.clone(),
                chunks: load_positions(
                    &tx,
                    &state.partner_id,
                    &state.partner_hash,
                    &right_positions,
                )?,
            };
            let edges = finish_pair(&left, &right, &accumulator, space)
                .map_err(crate::ai_boundary::core_failure)?;
            index::insert_candidate_edges(&tx, workspace, space, &edges, index::now_ms())?;
            edges_stored = edges.len();
        }
        tx.execute(
            "DELETE FROM ai_pair_progress WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
            params![workspace, space, job.document_id],
        )?;
        tx.execute(
            "UPDATE ai_relationship_coverage SET partner_cursor_seq = ?4, updated_at = ?5 WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
            params![workspace, space, job.document_id, state.partner_seq, index::now_ms().to_string()],
        )?;
    }
    tx.commit()?;
    Ok(Step::Tile {
        comparisons: cost,
        work,
        pair_done,
        edges: edges_stored,
    })
}

/// The hash and coverage re-check of a tile commit, inside its transaction:
/// both documents are still the revisions the tile was computed from, both are
/// still admitted at the same seq, and the stored pair progress (if any) is
/// exactly what this tile started from.
fn tile_is_current(
    tx: &Transaction<'_>,
    workspace: &str,
    space: &str,
    job: &Job,
    state: &PairState,
    cursor: i64,
) -> NativeResult<bool> {
    let admitted_at = |document_id: &str, hash: &str| -> NativeResult<Option<i64>> {
        Ok(tx
            .query_row(
                "SELECT r.seq FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id WHERE r.workspace_id = ?1 AND r.space_id = ?2 AND r.document_id = ?3 AND r.content_hash = ?4 AND d.content_hash = ?4 AND d.status = 'indexed'",
                params![workspace, space, document_id, hash],
                |row| row.get(0),
            )
            .optional()?)
    };
    if admitted_at(&job.document_id, &job.content_hash)? != Some(job.seq)
        || admitted_at(&state.partner_id, &state.partner_hash)? != Some(state.partner_seq)
    {
        return Ok(false);
    }
    let progress: Option<(String, i64, i64)> = tx
        .query_row(
            "SELECT partner_id, next_left, next_right FROM ai_pair_progress WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
            params![workspace, space, job.document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let cursor_unchanged = match progress {
        Some((partner_id, left, right)) => {
            state.stored
                && partner_id == state.partner_id
                && left as usize == state.next_left
                && right as usize == state.next_right
        }
        None => !state.stored,
    };
    let cursor_after: i64 = tx.query_row(
        "SELECT partner_cursor_seq FROM ai_relationship_coverage WHERE workspace_id = ?1 AND space_id = ?2 AND document_id = ?3",
        params![workspace, space, job.document_id],
        |row| row.get(0),
    )?;
    Ok(cursor_unchanged && cursor_after == cursor)
}

fn decode_vector(blob: &[u8], dimensions: usize) -> NativeResult<Vec<f32>> {
    if blob.len() % std::mem::size_of::<f32>() != 0 {
        return Err(error(
            ErrorCode::EvidenceInvalid,
            "A stored relationship vector is not a complete float array.",
        ));
    }
    let vector = blob
        .chunks_exact(std::mem::size_of::<f32>())
        .map(|bytes| f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect::<Vec<_>>();
    check_vector(&vector, dimensions)?;
    Ok(vector)
}

/// `limit` consecutive chunks of a document starting at `offset`, with their
/// vectors in `space`. `None` when the document changed or a vector is
/// missing, so the caller abandons the tile instead of comparing partial data.
fn load_chunks(
    conn: &Connection,
    document_id: &str,
    expected_hash: &str,
    offset: usize,
    limit: usize,
    space: &str,
    dimensions: usize,
) -> NativeResult<Option<Vec<RelationshipChunk>>> {
    let mut statement = conn.prepare(
        "SELECT d.content_hash, c.chunk_text, c.start_offset, c.end_offset, c.page, e.vector FROM chunks c JOIN documents d ON d.id = c.document_id LEFT JOIN embeddings e ON e.chunk_id = c.chunk_id AND e.space_id = ?2 WHERE c.document_id = ?1 ORDER BY c.ordinal LIMIT ?3 OFFSET ?4",
    )?;
    let rows = statement.query_map(
        params![document_id, space, limit as i64, offset as i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<u32>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        },
    )?;
    let mut chunks = Vec::new();
    for row in rows {
        let (hash, text, start, end, page, blob) = row?;
        if hash != expected_hash {
            return Ok(None);
        }
        let Some(blob) = blob else { return Ok(None) };
        if start < 0 || end <= start || (end - start) as usize != text.len() {
            return Err(error(
                ErrorCode::EvidenceInvalid,
                "A stored chunk has unusable UTF-8 byte offsets.",
            ));
        }
        chunks.push(RelationshipChunk {
            document_id: document_id.to_owned(),
            document_content_hash: hash,
            start: start as usize,
            end: end as usize,
            page,
            text,
            vector: decode_vector(&blob, dimensions)?,
        });
    }
    if chunks.len() != limit {
        return Ok(None);
    }
    Ok(Some(chunks))
}

/// The chunks at the given positions (without vectors), for building evidence.
fn load_positions(
    tx: &Transaction<'_>,
    document_id: &str,
    expected_hash: &str,
    positions: &[u32],
) -> NativeResult<BTreeMap<usize, RelationshipChunk>> {
    let mut chunks = BTreeMap::new();
    for position in positions {
        let row: Option<(String, String, i64, i64, Option<u32>)> = tx
            .query_row(
                "SELECT d.content_hash, c.chunk_text, c.start_offset, c.end_offset, c.page FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.document_id = ?1 ORDER BY c.ordinal LIMIT 1 OFFSET ?2",
                params![document_id, *position as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        let Some((hash, text, start, end, page)) = row else {
            return Err(error(
                ErrorCode::EvidenceInvalid,
                "A relationship evidence chunk is no longer available.",
            ));
        };
        if hash != expected_hash
            || start < 0
            || end <= start
            || (end - start) as usize != text.len()
        {
            return Err(error(
                ErrorCode::EvidenceInvalid,
                "A relationship evidence chunk changed.",
            ));
        }
        chunks.insert(
            *position as usize,
            RelationshipChunk {
                document_id: document_id.to_owned(),
                document_content_hash: hash,
                start: start as usize,
                end: end as usize,
                page,
                text,
                vector: Vec::new(),
            },
        );
    }
    Ok(chunks)
}

// ------------------------------------------------------------------ coverage

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverageState {
    /// No installed search model with a registered index: nothing to show.
    NoActiveSpace,
    /// Some indexed documents still lack vectors in the active space.
    EmbeddingIncomplete,
    /// Every indexed document is embedded but not every pair was compared.
    Partial,
    /// Every pair of currently embedded documents was compared.
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipCoverage {
    pub state: CoverageState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_fingerprint: Option<String>,
    pub eligible_documents: usize,
    pub indexed_documents: usize,
    pub pairs_considered: u64,
    pub pairs_remaining: u64,
    /// Documents whose stored candidates were truncated.
    pub overflow_documents: usize,
}

/// What Folio has compared in the active space, proven from the admission
/// order (see the module comment) and never from time or run counts.
///
/// `complete` holds iff every indexed document is eligible, every eligible
/// revision is admitted at its current hash, and no admitted job has a
/// partner left or an in-flight pair. By the ownership rule that means every
/// pair of currently eligible revisions was fully compared.
pub fn coverage(
    conn: &Connection,
    workspace_id: &str,
    space: Option<&str>,
) -> NativeResult<RelationshipCoverage> {
    let indexed: i64 = conn.query_row(
        "SELECT count(*) FROM documents d WHERE d.workspace_id = ?1 AND d.status = 'indexed' AND d.content_hash != '' AND EXISTS (SELECT 1 FROM chunks c WHERE c.document_id = d.id)",
        [workspace_id],
        |row| row.get(0),
    )?;
    let Some(space) = space else {
        return Ok(RelationshipCoverage {
            state: CoverageState::NoActiveSpace,
            space_fingerprint: None,
            eligible_documents: 0,
            indexed_documents: indexed as usize,
            pairs_considered: 0,
            pairs_remaining: 0,
            overflow_documents: 0,
        });
    };
    let eligible: i64 = conn.query_row(
        &format!("SELECT count(*) FROM documents d WHERE {ELIGIBLE_DOCUMENT}"),
        params![workspace_id, space],
        |row| row.get(0),
    )?;
    // Admitted at the document's current revision.
    let admitted: i64 = conn.query_row(
        "SELECT count(*) FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id AND d.content_hash = r.content_hash WHERE r.workspace_id = ?1 AND r.space_id = ?2",
        params![workspace_id, space],
        |row| row.get(0),
    )?;
    let unadmitted = (eligible - admitted).max(0);
    let partners_left: i64 = conn.query_row(
        "SELECT COALESCE(SUM((SELECT count(*) FROM ai_relationship_coverage p JOIN documents pd ON pd.id = p.document_id AND pd.content_hash = p.content_hash WHERE p.workspace_id = r.workspace_id AND p.space_id = r.space_id AND p.seq > r.partner_cursor_seq AND p.seq < r.seq)), 0) \
         FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id AND d.content_hash = r.content_hash WHERE r.workspace_id = ?1 AND r.space_id = ?2",
        params![workspace_id, space],
        |row| row.get(0),
    )?;
    let (admitted, unadmitted) = (admitted as u64, unadmitted as u64);
    let total_admitted_pairs = admitted * admitted.saturating_sub(1) / 2;
    let remaining = (partners_left as u64).min(total_admitted_pairs)
        + unadmitted * admitted
        + unadmitted * unadmitted.saturating_sub(1) / 2;
    let considered = total_admitted_pairs - (partners_left as u64).min(total_admitted_pairs);
    let overflow: i64 = conn.query_row(
        "SELECT count(*) FROM ai_relationship_coverage r JOIN documents d ON d.id = r.document_id AND d.content_hash = r.content_hash WHERE r.workspace_id = ?1 AND r.space_id = ?2 AND r.candidate_overflow = 1",
        params![workspace_id, space],
        |row| row.get(0),
    )?;
    let state = if indexed > eligible {
        CoverageState::EmbeddingIncomplete
    } else if remaining > 0 {
        CoverageState::Partial
    } else {
        CoverageState::Complete
    };
    Ok(RelationshipCoverage {
        state,
        space_fingerprint: Some(space.to_owned()),
        eligible_documents: eligible as usize,
        indexed_documents: indexed as usize,
        pairs_considered: considered,
        pairs_remaining: remaining,
        overflow_documents: overflow as usize,
    })
}
