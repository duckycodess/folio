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
}
