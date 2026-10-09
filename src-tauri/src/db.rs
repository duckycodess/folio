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
    include_str!("../migrations/006_collections.sql"),
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
}
