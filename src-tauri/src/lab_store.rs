//! Model Lab results in the existing `benchmark_results` table.
//!
//! Migration 001 created the table, and nothing else writes to it, so no new
//! migration is needed. Both JSON columns carry `schemaVersion` and a reader
//! refuses any other version. There are no foreign keys, so a row outlives the
//! model, runtime and workspace it was measured with. A review is appended to
//! the row's `reviews` and never replaces the measured output.

use crate::db::NativeResult;
use crate::error::{error, ErrorCode, FolioError};
use folio_core::error::{CoreError, CoreResult};
use folio_core::lab::{
    BenchmarkRecord, BenchmarkTask, LabSink, Review, ReviewStatus, RunStatus, RunSummary,
};
use folio_core::models::sha256_bytes;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

const SCHEMA_VERSION: u64 = 1;
const RUN_TASK: &str = "run";

/// Keys that live in `conditions_json`: what the measurement was made with.
const CONDITION_KEYS: &[&str] = &[
    "runId",
    "suite",
    "promptSha256",
    "model",
    "embeddingModelId",
    "runtimeDetail",
    "host",
    "conditions",
    "revision",
    "quantization",
    "runtime",
    "hardware",
    "contextTokens",
];

fn stored_error(message: &str) -> FolioError {
    error(ErrorCode::Internal, message)
}

fn unsupported_version(found: &Value) -> FolioError {
    stored_error("A stored Model Lab result uses a format this version of Folio cannot read.")
        .with_detail("reportedCode", "benchmarkSchemaVersion")
        .with_detail("found", found.to_string())
}

fn object(value: Value) -> NativeResult<Map<String, Value>> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(stored_error("A stored Model Lab result is not an object.")),
    }
}

fn check_version(map: &Map<String, Value>) -> NativeResult<()> {
    match map.get("schemaVersion") {
        Some(version) if version.as_u64() == Some(SCHEMA_VERSION) => Ok(()),
        other => Err(unsupported_version(other.unwrap_or(&Value::Null))),
    }
}

fn millis(value: u64) -> String {
    value.to_string()
}

/// Writes one record in its own transaction.
pub fn insert_record(conn: &mut Connection, record: &BenchmarkRecord) -> NativeResult<()> {
    record.validate().map_err(|cause| {
        stored_error("A Model Lab record was not valid.").with_detail("cause", cause.to_string())
    })?;
    let mut all = object(serde_json::to_value(record)?)?;
    let id = all
        .remove("id")
        .and_then(|v| v.as_str().map(str::to_string));
    let case_id = all
        .remove("caseId")
        .and_then(|v| v.as_str().map(str::to_string));
    let task = all
        .remove("task")
        .and_then(|v| v.as_str().map(str::to_string));
    let model_id = all
        .remove("modelId")
        .and_then(|v| v.as_str().map(str::to_string));
    let (Some(id), Some(case_id), Some(task), Some(model_id)) = (id, case_id, task, model_id)
    else {
        return Err(stored_error("A Model Lab record is missing its identity."));
    };

    let mut conditions = Map::new();
    let mut measurements = Map::new();
    conditions.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
    measurements.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
    for (key, value) in all {
        if key == "schemaVersion" {
            continue;
        }
        if CONDITION_KEYS.contains(&key.as_str()) {
            conditions.insert(key, value);
        } else {
            measurements.insert(key, value);
        }
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO benchmark_results (id, case_id, task_type, model_id, conditions_json, measurements_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            case_id,
            task,
            model_id,
            serde_json::to_string(&conditions)?,
            serde_json::to_string(&measurements)?,
            millis(record.created_at),
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Inserts a run's summary row, or updates it as its status changes.
pub fn upsert_run(conn: &mut Connection, run: &RunSummary) -> NativeResult<()> {
    let conditions = json!({
        "schemaVersion": SCHEMA_VERSION,
        "runId": run.run_id,
        "requestedModelIds": run.requested_model_ids,
        "suite": run.suite,
        "corpusSha256": run.corpus_sha256,
        "host": run.host,
        "serverSettings": run.server_settings,
    });
    let mut measurements = json!({
        "schemaVersion": SCHEMA_VERSION,
        "status": run.status,
        "startedAt": run.started_at,
        "endedAt": run.ended_at,
        "indexBuildMs": run.index_build_ms,
    });
    if let Some(message) = &run.error {
        measurements["error"] = json!(message);
    }
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO benchmark_results (id, case_id, task_type, model_id, conditions_json, measurements_json, created_at) VALUES (?1, '', ?2, '', ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET conditions_json = excluded.conditions_json, measurements_json = excluded.measurements_json",
        params![
            run.run_id,
            RUN_TASK,
            serde_json::to_string(&conditions)?,
            serde_json::to_string(&measurements)?,
            millis(run.started_at),
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// A run still marked `running` when no run can be in progress was interrupted
/// (Folio closed or crashed). Its finished cases stay; the run says it did not
/// complete. Returns how many runs were marked.
pub fn fail_interrupted_runs(conn: &mut Connection, now_ms: u64) -> NativeResult<usize> {
    let mut marked = 0;
    for mut run in list_runs(conn)? {
        if run.status != RunStatus::Running {
            continue;
        }
        run.status = RunStatus::Failed;
        run.ended_at = Some(now_ms);
        run.error = Some("Folio stopped before this run finished.".into());
        upsert_run(conn, &run)?;
        marked += 1;
    }
    Ok(marked)
}

#[derive(Clone, Debug, Default)]
pub struct RecordFilter {
    pub run_id: Option<String>,
    pub model_id: Option<String>,
    pub task: Option<BenchmarkTask>,
}

fn rebuild_record(
    id: String,
    case_id: String,
    task: String,
    model_id: String,
    conditions_json: String,
    measurements_json: String,
) -> NativeResult<BenchmarkRecord> {
    let conditions = object(serde_json::from_str(&conditions_json)?)?;
    let measurements = object(serde_json::from_str(&measurements_json)?)?;
    check_version(&conditions)?;
    check_version(&measurements)?;
    let mut all = Map::new();
    all.extend(conditions);
    all.extend(measurements);
    all.insert("id".into(), json!(id));
    all.insert("caseId".into(), json!(case_id));
    all.insert("task".into(), json!(task));
    all.insert("modelId".into(), json!(model_id));
    let record: BenchmarkRecord = serde_json::from_value(Value::Object(all))?;
    record.validate().map_err(|cause| {
        stored_error("A stored Model Lab result is not valid.")
            .with_detail("cause", cause.to_string())
    })?;
    Ok(record)
}

/// Records in the order they were measured.
pub fn list_records(
    conn: &Connection,
    filter: &RecordFilter,
) -> NativeResult<Vec<BenchmarkRecord>> {
    let mut statement = conn.prepare(
        "SELECT id, case_id, task_type, model_id, conditions_json, measurements_json FROM benchmark_results WHERE task_type != ?1 AND (?2 IS NULL OR model_id = ?2) AND (?3 IS NULL OR task_type = ?3) ORDER BY CAST(created_at AS INTEGER), rowid",
    )?;
    let rows = statement.query_map(
        params![
            RUN_TASK,
            filter.model_id,
            filter.task.map(|task| task.as_str())
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;
    let mut records = Vec::new();
    for row in rows {
        let (id, case_id, task, model_id, conditions, measurements) = row?;
        // One row this version can't read (a newer schema after a downgrade,
        // or a rule added later) is left out, not allowed to hide every result.
        let record = match rebuild_record(id, case_id, task, model_id, conditions, measurements)
        {
            Ok(record) => record,
            Err(failure) => {
                skipped_row("result", &failure);
                continue;
            }
        };
        if filter
            .run_id
            .as_deref()
            .is_some_and(|run| run != record.run_id)
        {
            continue;
        }
        records.push(record);
    }
    Ok(records)
}

pub fn list_runs(conn: &Connection) -> NativeResult<Vec<RunSummary>> {
    let mut statement = conn.prepare(
        "SELECT conditions_json, measurements_json FROM benchmark_results WHERE task_type = ?1 ORDER BY CAST(created_at AS INTEGER), rowid",
    )?;
    let rows = statement.query_map(params![RUN_TASK], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut runs = Vec::new();
    for row in rows {
        let (conditions_json, measurements_json) = row?;
        match rebuild_run(&conditions_json, &measurements_json) {
            Ok(run) => runs.push(run),
            Err(failure) => skipped_row("run", &failure),
        }
    }
    Ok(runs)
}

fn rebuild_run(conditions_json: &str, measurements_json: &str) -> NativeResult<RunSummary> {
    let conditions = object(serde_json::from_str(conditions_json)?)?;
    let measurements = object(serde_json::from_str(measurements_json)?)?;
    check_version(&conditions)?;
    check_version(&measurements)?;
    let mut all = Map::new();
    all.extend(conditions);
    all.extend(measurements);
    Ok(serde_json::from_value(Value::Object(all))?)
}

fn skipped_row(kind: &str, failure: &FolioError) {
    eprintln!(
        "Model Lab left out a stored {kind} this version can't read: {}",
        failure.message
    );
}

pub struct ReviewInput {
    pub status: ReviewStatus,
    pub reviewer: String,
    pub notes: Option<String>,
}

/// Appends a person's review to one record. It names the output hash the
/// reviewer read; if the stored output no longer matches that hash, or its own
/// hash, nothing is written. The measured output and `correctness` are never
/// touched.
pub fn record_review(
    conn: &mut Connection,
    id: &str,
    output_sha256: &str,
    input: ReviewInput,
    now_ms: u64,
) -> NativeResult<BenchmarkRecord> {
    if input.reviewer.trim().is_empty() {
        return Err(error(
            ErrorCode::EvidenceInvalid,
            "A review needs a reviewer's name.",
        ));
    }
    let tx = conn.transaction()?;
    let row = tx
        .query_row(
            "SELECT case_id, task_type, model_id, conditions_json, measurements_json FROM benchmark_results WHERE id = ?1 AND task_type != ?2",
            params![id, RUN_TASK],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((case_id, task, model_id, conditions_json, measurements_json)) = row else {
        return Err(error(
            ErrorCode::EvidenceInvalid,
            "That Model Lab result no longer exists.",
        )
        .with_detail("resultId", id));
    };
    let mut measurements = object(serde_json::from_str(&measurements_json)?)?;
    check_version(&measurements)?;
    let stored_hash = measurements
        .get("outputSha256")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let recomputed = sha256_bytes(&serde_json::to_vec(
        measurements.get("output").unwrap_or(&Value::Null),
    )?);
    if stored_hash != output_sha256 || recomputed != stored_hash {
        return Err(error(
            ErrorCode::EvidenceInvalid,
            "The recorded output is not the one that was reviewed. Open the result again before reviewing it.",
        )
        .with_detail("resultId", id));
    }
    let review = Review {
        status: input.status,
        reviewer: input.reviewer.trim().to_string(),
        reviewed_at: now_ms,
        notes: input.notes.filter(|notes| !notes.trim().is_empty()),
        output_sha256: stored_hash,
    };
    match measurements.get_mut("reviews") {
        Some(Value::Array(reviews)) => reviews.push(serde_json::to_value(&review)?),
        _ => {
            return Err(stored_error(
                "A stored Model Lab result has no review list.",
            ))
        }
    }
    let measurements_json = serde_json::to_string(&measurements)?;
    tx.execute(
        "UPDATE benchmark_results SET measurements_json = ?1 WHERE id = ?2",
        params![measurements_json, id],
    )?;
    let record = rebuild_record(
        id.to_string(),
        case_id,
        task,
        model_id,
        conditions_json,
        measurements_json,
    )?;
    tx.commit()?;
    Ok(record)
}

/// The lab's sink over its own connection to the index database.
pub struct SqliteLabSink {
    conn: Connection,
}

impl SqliteLabSink {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

fn to_core(failure: FolioError) -> CoreError {
    CoreError::Message(failure.message)
}

impl LabSink for SqliteLabSink {
    fn record(&mut self, record: &BenchmarkRecord) -> CoreResult<()> {
        insert_record(&mut self.conn, record).map_err(to_core)
    }

    fn run_status(&mut self, run: &RunSummary) -> CoreResult<()> {
        upsert_run(&mut self.conn, run).map_err(to_core)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use folio_core::lab::{RequestPosition, SchemaVersion};
    use folio_core::models::ModelStore;
    use std::path::Path;

    const GOLDEN: &str = include_str!("../../fixtures/contracts/benchmark-record.json");

    fn sample(id: &str, created_at: u64) -> BenchmarkRecord {
        let mut record: BenchmarkRecord = serde_json::from_str(GOLDEN).unwrap();
        record.id = id.into();
        record.created_at = created_at;
        record
    }

    fn run(status: RunStatus) -> RunSummary {
        RunSummary {
            run_id: "run-1".into(),
            status,
            requested_model_ids: vec!["embedder".into(), "generator".into()],
            suite: sample("x", 0).suite,
            corpus_sha256: "c".repeat(64),
            host: sample("x", 0).host,
            server_settings: sample("x", 0).server_settings.unwrap(),
            started_at: 1_000,
            ended_at: None,
            index_build_ms: Some(12),
            error: None,
            schema_version: SchemaVersion,
        }
    }

    fn database(dir: &Path) -> Connection {
        // The real migrations, on a real file.
        db::open(&dir.join("folio.sqlite")).unwrap()
    }

    fn edit_measurements(conn: &Connection, edit: impl Fn(&mut Value)) {
        let stored: String = conn
            .query_row(
                "SELECT measurements_json FROM benchmark_results LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut value: Value = serde_json::from_str(&stored).unwrap();
        edit(&mut value);
        conn.execute(
            "UPDATE benchmark_results SET measurements_json = ?1",
            params![serde_json::to_string(&value).unwrap()],
        )
        .unwrap();
    }

    #[test]
    fn records_and_runs_survive_closing_and_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        let first = sample("rec-1", 2_000);
        let mut second = sample("rec-2", 3_000);
        second.timing.request_position = RequestPosition::ImmediateRepeat;
        second.timing.requests_since_process_start = 2;
        second.cold = false;
        upsert_run(&mut conn, &run(RunStatus::Running)).unwrap();
        insert_record(&mut conn, &second).unwrap();
        insert_record(&mut conn, &first).unwrap();
        let mut finished = run(RunStatus::Completed);
        finished.ended_at = Some(9_000);
        upsert_run(&mut conn, &finished).unwrap();
        drop(conn);

        let conn = database(dir.path());
        let records = list_records(&conn, &RecordFilter::default()).unwrap();
        assert_eq!(records, vec![first, second], "listed in measured order");
        let runs = list_runs(&conn).unwrap();
        assert_eq!(runs, vec![finished], "one row per run, updated in place");
        let count: i64 = conn
            .query_row("SELECT count(*) FROM benchmark_results", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn the_existing_table_is_used_without_a_schema_change() {
        let dir = tempfile::tempdir().unwrap();
        let conn = database(dir.path());
        let columns: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('benchmark_results') ORDER BY cid")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|name| name.unwrap())
            .collect();
        assert_eq!(
            columns,
            [
                "id",
                "case_id",
                "task_type",
                "model_id",
                "conditions_json",
                "measurements_json",
                "created_at"
            ]
        );
    }

    #[test]
    fn both_json_columns_are_versioned_and_keep_the_conditions_and_output() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        insert_record(&mut conn, &sample("rec-1", 2_000)).unwrap();
        let (task, conditions, measurements): (String, String, String) = conn
            .query_row(
                "SELECT task_type, conditions_json, measurements_json FROM benchmark_results WHERE id = 'rec-1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(task, "summary");
        let conditions: Value = serde_json::from_str(&conditions).unwrap();
        let measurements: Value = serde_json::from_str(&measurements).unwrap();
        assert_eq!(conditions["schemaVersion"], 1);
        assert_eq!(measurements["schemaVersion"], 1);
        assert_eq!(conditions["model"]["revision"], "contract-example-revision");
        assert_eq!(conditions["conditions"]["pageCache"], "notControlled");
        assert!(measurements["output"].is_object());
        assert!(measurements["outputSha256"].is_string());
        assert_eq!(measurements["reviews"], json!([]));
    }

    #[test]
    fn a_stored_format_this_version_cannot_read_is_refused_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        insert_record(&mut conn, &sample("rec-1", 2_000)).unwrap();
        edit_measurements(&conn, |value| value["schemaVersion"] = json!(2));
        insert_record(&mut conn, &sample("rec-2", 3_000)).unwrap();

        // The row is left out of the list, never guessed at, and the rest
        // still list.
        let listed = list_records(&conn, &RecordFilter::default()).unwrap();
        let ids: Vec<&str> = listed.iter().map(|record| record.id.as_str()).collect();
        assert_eq!(ids, ["rec-2"]);

        // Reading that row itself is refused.
        let failure = record_review(
            &mut conn,
            "rec-1",
            "sha256:unused",
            ReviewInput {
                status: ReviewStatus::Correct,
                reviewer: "TJ".into(),
                notes: None,
            },
            5_000,
        )
        .unwrap_err();
        assert_eq!(failure.code, ErrorCode::Internal);
        assert_eq!(
            failure.detail("reportedCode"),
            Some("benchmarkSchemaVersion")
        );
    }

    #[test]
    fn a_run_this_version_cannot_read_does_not_stop_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        upsert_run(&mut conn, &run(RunStatus::Running)).unwrap();
        // A run row from a newer Folio, written after a downgrade.
        conn.execute(
            "UPDATE benchmark_results SET measurements_json = json_set(measurements_json, '$.schemaVersion', 2)",
            [],
        )
        .unwrap();
        let mut current = run(RunStatus::Running);
        current.run_id = "run-2".into();
        upsert_run(&mut conn, &current).unwrap();

        assert_eq!(list_runs(&conn).unwrap().len(), 1);
        // Marking interrupted runs, which a new run does first, still works.
        assert_eq!(fail_interrupted_runs(&mut conn, 9_000).unwrap(), 1);
    }

    #[test]
    fn a_review_is_appended_bound_to_the_output_and_never_changes_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        let mut original = sample("rec-1", 2_000);
        // Make the stored hash the real hash of the stored output.
        original.output_sha256 = sha256_bytes(&serde_json::to_vec(&original.output).unwrap());
        insert_record(&mut conn, &original).unwrap();

        let reviewed = record_review(
            &mut conn,
            "rec-1",
            &original.output_sha256,
            ReviewInput {
                status: ReviewStatus::PartiallyCorrect,
                reviewer: " TJ ".into(),
                notes: Some("Missed the presentation date.".into()),
            },
            5_000,
        )
        .unwrap();
        assert_eq!(reviewed.reviews.len(), 1);
        assert_eq!(reviewed.reviews[0].reviewer, "TJ");
        assert_eq!(reviewed.reviews[0].output_sha256, original.output_sha256);
        assert_eq!(reviewed.output, original.output);
        assert_eq!(reviewed.output_sha256, original.output_sha256);
        assert_eq!(
            reviewed.correctness, None,
            "a review never writes correctness"
        );

        record_review(
            &mut conn,
            "rec-1",
            &original.output_sha256,
            ReviewInput {
                status: ReviewStatus::Correct,
                reviewer: "TJ".into(),
                notes: None,
            },
            6_000,
        )
        .unwrap();
        drop(conn);
        let conn = database(dir.path());
        let stored = &list_records(&conn, &RecordFilter::default()).unwrap()[0];
        assert_eq!(stored.reviews.len(), 2, "reviews accumulate");
        assert_eq!(stored.reviews[0].status, ReviewStatus::PartiallyCorrect);
        assert_eq!(stored.reviews[1].status, ReviewStatus::Correct);
        assert_eq!(stored.output, original.output);
    }

    #[test]
    fn a_review_of_a_different_output_is_refused_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        let mut original = sample("rec-1", 2_000);
        original.output_sha256 = sha256_bytes(&serde_json::to_vec(&original.output).unwrap());
        insert_record(&mut conn, &original).unwrap();
        let input = || ReviewInput {
            status: ReviewStatus::Correct,
            reviewer: "TJ".into(),
            notes: None,
        };

        let stale = record_review(&mut conn, "rec-1", &"9".repeat(64), input(), 1).unwrap_err();
        assert_eq!(stale.code, ErrorCode::EvidenceInvalid);
        let missing =
            record_review(&mut conn, "nope", &original.output_sha256, input(), 1).unwrap_err();
        assert_eq!(missing.code, ErrorCode::EvidenceInvalid);
        let nameless = record_review(
            &mut conn,
            "rec-1",
            &original.output_sha256,
            ReviewInput {
                status: ReviewStatus::Correct,
                reviewer: "  ".into(),
                notes: None,
            },
            1,
        )
        .unwrap_err();
        assert_eq!(nameless.code, ErrorCode::EvidenceInvalid);

        // A stored output that no longer matches its own hash cannot be reviewed.
        edit_measurements(&conn, |value| {
            value["output"]["text"] = json!("edited later")
        });
        let tampered =
            record_review(&mut conn, "rec-1", &original.output_sha256, input(), 1).unwrap_err();
        assert_eq!(tampered.code, ErrorCode::EvidenceInvalid);
        let stored = &list_records(&conn, &RecordFilter::default()).unwrap()[0];
        assert!(stored.reviews.is_empty());
    }

    #[test]
    fn results_remain_after_the_model_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("app-data");
        let model_dir = data.join("models").join("qwen3-0.6b-q4-k-m");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("model.gguf"), b"weights").unwrap();

        let mut conn = database(&data);
        let mut record = sample("rec-1", 2_000);
        record.model_id = "qwen3-0.6b-q4-k-m".into();
        record.model.id = "qwen3-0.6b-q4-k-m".into();
        insert_record(&mut conn, &record).unwrap();
        drop(conn);

        let store = ModelStore::new(&data).unwrap();
        store.remove_model("qwen3-0.6b-q4-k-m").unwrap();
        assert!(!model_dir.exists());

        let conn = database(&data);
        let records = list_records(
            &conn,
            &RecordFilter {
                model_id: Some("qwen3-0.6b-q4-k-m".into()),
                ..RecordFilter::default()
            },
        )
        .unwrap();
        assert_eq!(records, vec![record]);
        assert_eq!(records[0].model.revision, "contract-example-revision");
    }

    #[test]
    fn records_filter_by_run_model_and_task_and_runs_are_never_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        upsert_run(&mut conn, &run(RunStatus::Completed)).unwrap();
        let mut a = sample("a", 1);
        a.run_id = "run-1".into();
        let mut b = sample("b", 2);
        b.run_id = "run-2".into();
        b.model_id = "other".into();
        b.model.id = "other".into();
        b.task = BenchmarkTask::Interpretation;
        b.correctness = Some(true);
        insert_record(&mut conn, &a).unwrap();
        insert_record(&mut conn, &b).unwrap();

        let all = list_records(&conn, &RecordFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        let run_two = RecordFilter {
            run_id: Some("run-2".into()),
            ..RecordFilter::default()
        };
        assert_eq!(list_records(&conn, &run_two).unwrap(), vec![b.clone()]);
        let by_model = RecordFilter {
            model_id: Some("contract-example-model".into()),
            ..RecordFilter::default()
        };
        assert_eq!(list_records(&conn, &by_model).unwrap(), vec![a]);
        let by_task = RecordFilter {
            task: Some(BenchmarkTask::Interpretation),
            ..RecordFilter::default()
        };
        assert_eq!(list_records(&conn, &by_task).unwrap(), vec![b]);
    }

    #[test]
    fn an_invalid_record_is_refused_before_it_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        let mut record = sample("rec-1", 1);
        record.correctness = Some(true); // a summary is never graded
        assert!(insert_record(&mut conn, &record).is_err());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM benchmark_results", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn a_run_left_running_is_marked_interrupted_and_keeps_its_cases() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        upsert_run(&mut conn, &run(RunStatus::Running)).unwrap();
        insert_record(&mut conn, &sample("rec-1", 2_000)).unwrap();
        assert_eq!(fail_interrupted_runs(&mut conn, 7_000).unwrap(), 1);
        assert_eq!(fail_interrupted_runs(&mut conn, 8_000).unwrap(), 0);
        let runs = list_runs(&conn).unwrap();
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert_eq!(runs[0].ended_at, Some(7_000));
        assert!(runs[0].error.as_deref().unwrap().contains("stopped"));
        assert_eq!(
            list_records(&conn, &RecordFilter::default()).unwrap().len(),
            1
        );
    }

    #[test]
    fn the_sqlite_sink_writes_what_the_harness_reports() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = SqliteLabSink::new(database(dir.path()));
        sink.run_status(&run(RunStatus::Running)).unwrap();
        sink.record(&sample("rec-1", 2_000)).unwrap();
        let mut bad = sample("rec-2", 2_001);
        bad.correctness = Some(false);
        assert!(sink.record(&bad).is_err());
        drop(sink);
        let conn = database(dir.path());
        assert_eq!(
            list_records(&conn, &RecordFilter::default()).unwrap().len(),
            1
        );
        assert_eq!(list_runs(&conn).unwrap().len(), 1);
    }

    #[test]
    fn no_stored_key_names_an_aggregate_or_score() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = database(dir.path());
        insert_record(&mut conn, &sample("rec-1", 1)).unwrap();
        upsert_run(&mut conn, &run(RunStatus::Completed)).unwrap();
        fn keys(value: &Value, found: &mut Vec<String>) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        found.push(key.clone());
                        keys(child, found);
                    }
                }
                Value::Array(items) => items.iter().for_each(|item| keys(item, found)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        let mut statement = conn
            .prepare("SELECT conditions_json, measurements_json FROM benchmark_results")
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap();
        for row in rows {
            let (conditions, measurements) = row.unwrap();
            keys(&serde_json::from_str(&conditions).unwrap(), &mut found);
            keys(&serde_json::from_str(&measurements).unwrap(), &mut found);
        }
        let aggregate: Vec<&String> = found
            .iter()
            .filter(|key| {
                let key = key.to_lowercase();
                key.contains("score") || key.contains("aggregate") || key.contains("overall")
            })
            .collect();
        assert!(aggregate.is_empty(), "{aggregate:?}");
    }
}
