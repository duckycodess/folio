use std::path::Path;
use rusqlite::Connection;
use crate::error::{error, ErrorCode, FolioError};

pub type NativeResult<T> = Result<T, FolioError>;

/// Applied in order; `PRAGMA user_version` records how many have run.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_initial.sql"),
    include_str!("../migrations/002_index_state.sql"),
    include_str!("../migrations/003_actions.sql"),
    include_str!("../migrations/004_retry_backoff.sql"),
    include_str!("../migrations/005_delete_history.sql"),
    include_str!("../migrations/006_activity.sql"),
    include_str!("../migrations/007_ai_relationships.sql"),
    include_str!("../migrations/008_ai_relationship_coverage.sql"),
];

impl From<rusqlite::Error> for FolioError {
    fn from(cause: rusqlite::Error) -> Self {
        error(ErrorCode::Internal, "Folio's local index could not be read or updated.").with_detail("cause", cause.to_string())
    }
}

impl From<std::io::Error> for FolioError {
    fn from(cause: std::io::Error) -> Self {
        error(ErrorCode::DocumentUnavailable, "A file could not be read.").with_detail("cause", cause.to_string())
    }
}

impl From<serde_json::Error> for FolioError {
    fn from(cause: serde_json::Error) -> Self {
        error(ErrorCode::Internal, "Stored index data could not be decoded.").with_detail("cause", cause.to_string())
    }
}

pub fn open(path: &Path) -> NativeResult<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    prepare(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> NativeResult<Connection> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(mut conn: Connection) -> NativeResult<Connection> {
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> NativeResult<()> {
    let applied: usize = conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as usize;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_enable_fts5() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("folio.sqlite");
        drop(open(&path).unwrap());
        let conn = open(&path).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        let fts: i64 = conn.query_row("SELECT count(*) FROM chunks_fts", [], |row| row.get(0)).unwrap();
        assert_eq!(fts, 0);
    }

    #[test]
    fn migration_005_keeps_existing_history_and_accepts_deletions() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..4] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 4).unwrap();
        conn.execute_batch(
            "INSERT INTO workspaces (id, root_path, authorized_at) VALUES ('w', '/w', '0');
             INSERT INTO action_plans (id, workspace_id, plan_json, plan_digest, status, created_at, expires_at, applied_at) VALUES ('p', 'w', '{}', 'd', 'approved', '0', '1', '0');
             INSERT INTO history (id, plan_id, operation_index, operation_kind, document_ref, before_path, after_path, before_hash, after_hash, before_content, applied_at, undone_at, recoverable) VALUES ('h0', 'p', 0, 'edit', 'w:a.md', 'a.md', 'a.md', 'sha256:a', 'sha256:b', X'4F6B74', '5', NULL, 1);
             INSERT INTO history (id, plan_id, operation_index, operation_kind, document_ref, before_path, after_path, before_hash, after_hash, applied_at, undone_at, recoverable) VALUES ('h1', 'p', 1, 'rename', 'w:b.md', 'b.md', 'c.md', 'sha256:c', 'sha256:c', '5', '6', 1);",
        )
        .unwrap();
        assert!(conn.execute("INSERT INTO history (id, plan_id, operation_kind, applied_at) VALUES ('early', 'p', 'delete', '5')", []).is_err(), "version 4 refuses a deletion");

        migrate(&mut conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        type Row = (String, i64, String, Option<String>, Option<String>, Option<String>, Option<Vec<u8>>, Option<String>, i64);
        let rows: Vec<Row> = conn
            .prepare("SELECT id, operation_index, operation_kind, document_ref, after_path, after_hash, before_content, undone_at, recoverable FROM history ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            rows,
            vec![
                ("h0".into(), 0, "edit".into(), Some("w:a.md".into()), Some("a.md".into()), Some("sha256:b".into()), Some(b"Okt".to_vec()), None, 1),
                ("h1".into(), 1, "rename".into(), Some("w:b.md".into()), Some("c.md".into()), Some("sha256:c".into()), None, Some("6".into()), 1),
            ]
        );
        conn.execute("INSERT INTO history (id, plan_id, operation_index, operation_kind, document_ref, before_path, before_hash, before_content, applied_at) VALUES ('h2', 'p', 2, 'delete', 'w:d.md', 'd.md', 'sha256:d', X'00', '7')", []).unwrap();
        assert!(conn.execute("INSERT INTO history (id, plan_id, operation_kind, applied_at) VALUES ('h3', 'p', 'erase', '7')", []).is_err(), "other kinds are still refused");
        assert!(conn.execute("INSERT INTO history (id, plan_id, operation_kind, applied_at) VALUES ('h4', 'missing-plan', 'delete', '7')", []).is_err(), "history still belongs to a recorded plan");
        let defaults: (i64, String, i64) = conn.query_row("INSERT INTO history (id, plan_id, applied_at) VALUES ('h5', 'p', '8') RETURNING operation_index, operation_kind, recoverable", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
        assert_eq!(defaults, (0, "edit".into(), 1));
        let indexed: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'index' AND name = 'history_plan_idx' AND tbl_name = 'history'", [], |row| row.get(0)).unwrap();
        assert_eq!(indexed, 1);
    }

    fn seed_documents(conn: &Connection) {
        conn.execute_batch(
            "INSERT INTO workspaces (id, root_path, authorized_at) VALUES ('w', '/w', '0');
             INSERT INTO documents (id, workspace_id, relative_path, content_hash, media_type, size_bytes, modified_at) VALUES ('a', 'w', 'a.md', 'sha256:a', 'text/markdown', 1, '0');
             INSERT INTO documents (id, workspace_id, relative_path, content_hash, media_type, size_bytes, modified_at) VALUES ('b', 'w', 'b.md', 'sha256:b', 'text/markdown', 1, '0');
             INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES ('link', 'a', 'b', 'explicitReference', '{}', 'documentLink', NULL, 'sha256:a', 'sha256:b', '0');",
        )
        .unwrap();
    }

    fn assert_coverage_schema(conn: &Connection) {
        for table in ["ai_relationship_seq", "ai_relationship_coverage", "ai_pair_progress", "ai_relationship_schedule"] {
            let exists: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |row| row.get(0)).unwrap();
            assert_eq!(exists, 1, "{table} exists");
        }
        let link: (String, Option<String>, Option<f64>, Option<f64>) = conn
            .query_row("SELECT relationship_type, space_fingerprint, score, discovery_cosine FROM relationships WHERE id = 'link'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
            .unwrap();
        assert_eq!(link, ("explicitReference".into(), None, None, None), "an existing link keeps its shape");
        let violations: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).unwrap();
        assert_eq!(violations, 0);
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[test]
    fn migration_006_keeps_earlier_plans_as_unknown_and_rebuilds_them_from_history() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        // A two-operation plan from before 006 that stopped at its second operation.
        conn.execute_batch(
            r#"INSERT INTO workspaces (id, root_path, authorized_at) VALUES ('w', '/w', '0');
             INSERT INTO action_plans (id, workspace_id, plan_json, plan_digest, status, created_at, expires_at, applied_at, stop_reason) VALUES ('p', 'w', '{"operations":[{"kind":"rename","relativePath":"b.md","destinationRelativePath":"c.md"},{"kind":"edit","relativePath":"a.md"}],"impacts":[]}', 'd', 'failed', '0', '1', '5', 'failed');
             INSERT INTO history (id, plan_id, operation_index, operation_kind, document_ref, before_path, after_path, before_hash, after_hash, applied_at, recoverable) VALUES ('h0', 'p', 0, 'rename', 'w:b.md', 'b.md', 'c.md', 'sha256:c', 'sha256:c', '5', 1);"#,
        )
        .unwrap();

        migrate(&mut conn).unwrap();
        let (source, outcome): (String, Option<String>) = conn.query_row("SELECT source, outcome_json FROM action_plans WHERE id = 'p'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!((source.as_str(), outcome), ("unknown", None));
        assert!(conn.execute("UPDATE action_plans SET source = 'document' WHERE id = 'p'", []).is_err(), "only the closed list of sources is stored");

        let batches = crate::writer::list_activity(&conn, "w", 10, None).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(batch.source, crate::contracts::PlanSource::Unknown);
        assert_eq!(batch.stop_reason, Some(crate::contracts::BatchStopReason::Failed));
        assert_eq!(batch.finished_at, None);
        assert_eq!(batch.operations.len(), 2);
        // The rename left history, so it succeeded; the edit's outcome was never
        // recorded, so it has no status rather than an invented failure.
        assert_eq!(batch.operations[0].status, Some(crate::contracts::OperationStatus::Succeeded));
        assert_eq!(batch.operations[0].history.as_ref().map(|entry| entry.id.as_str()), Some("h0"));
        assert_eq!(batch.operations[1].status, None);
        assert_eq!(batch.operations[1].before_relative_path.as_deref(), Some("a.md"));
        assert!(batch.operations[1].error.is_none() && batch.operations[1].history.is_none());
    }

    #[test]
    fn migration_008_applies_to_a_populated_version_5_database() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..5] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        seed_documents(&conn);
        migrate(&mut conn).unwrap();
        assert_coverage_schema(&conn);
    }

    #[test]
    fn migration_008_applies_to_a_populated_version_7_database_with_ai_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        for sql in &MIGRATIONS[..7] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 7).unwrap();
        seed_documents(&conn);
        conn.execute_batch(
            "INSERT INTO embedding_spaces (id, model_id, revision, quantization, dimensions, preprocessing_fingerprint) VALUES ('s', 'm', 'r', 'q', 2, 'p');
             INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at, space_fingerprint, score) VALUES ('sim', 'a', 'b', 'similarity', '{}', 'embedding', NULL, 'sha256:a', 'sha256:b', '0', 's', 0.9);",
        )
        .unwrap();
        migrate(&mut conn).unwrap();
        assert_coverage_schema(&conn);
        let ai: (Option<String>, Option<f64>, Option<f64>) = conn
            .query_row("SELECT space_fingerprint, score, discovery_cosine FROM relationships WHERE id = 'sim'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap();
        assert_eq!(ai, (Some("s".into()), Some(0.9), None), "an existing AI row keeps its space and score");
    }

    #[test]
    fn coverage_rows_cascade_with_documents_and_spaces() {
        let conn = crate::db::open_in_memory().unwrap();
        seed_documents(&conn);
        conn.execute_batch(
            "INSERT INTO embedding_spaces (id, model_id, revision, quantization, dimensions, preprocessing_fingerprint) VALUES ('s', 'm', 'r', 'q', 2, 'p');
             INSERT INTO ai_relationship_seq (workspace_id, space_id, next_seq) VALUES ('w', 's', 3);
             INSERT INTO ai_relationship_coverage (workspace_id, space_id, document_id, content_hash, seq, updated_at) VALUES ('w', 's', 'a', 'sha256:a', 1, '0'), ('w', 's', 'b', 'sha256:b', 2, '0');
             INSERT INTO ai_pair_progress (workspace_id, space_id, document_id, partner_id, document_hash, partner_hash, partner_seq, next_left, next_right, accumulator_json) VALUES ('w', 's', 'b', 'a', 'sha256:b', 'sha256:a', 1, 0, 0, '{}');",
        )
        .unwrap();
        assert!(conn.execute("INSERT INTO ai_relationship_coverage (workspace_id, space_id, document_id, content_hash, seq, updated_at) VALUES ('w', 's', 'a', 'x', 9, '0')", []).is_err(), "one coverage row per document");
        assert!(conn.execute("INSERT INTO ai_relationship_coverage (workspace_id, space_id, document_id, content_hash, seq, updated_at) VALUES ('w', 's', 'a2', 'x', 2, '0')", []).is_err(), "seq is unique per space");
        conn.execute("DELETE FROM documents WHERE id = 'a'", []).unwrap();
        let count = |table: &str| -> i64 { conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap() };
        assert_eq!(count("ai_relationship_coverage"), 1, "the deleted document's coverage left");
        assert_eq!(count("ai_pair_progress"), 0, "a pair naming the deleted document left");
        let next: i64 = conn.query_row("SELECT next_seq FROM ai_relationship_seq", [], |row| row.get(0)).unwrap();
        assert_eq!(next, 3, "deletion never decrements the admission counter");
        conn.execute("DELETE FROM embedding_spaces WHERE id = 's'", []).unwrap();
        assert_eq!(count("ai_relationship_coverage"), 0);
        assert_eq!(count("ai_relationship_seq"), 0);
    }
}
