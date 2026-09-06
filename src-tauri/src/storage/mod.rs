use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

pub mod chunks;
pub mod copilots;
pub mod documents;
pub mod exchanges;
pub mod facts;
pub mod instructions;
pub mod knowledge;
mod migrations;
pub mod past_sessions;
pub mod sessions;
pub mod settings;
pub mod suggestions;
pub mod topics;
pub mod transcripts;
pub mod window_summaries;

/// The app's single SQLite connection, managed as Tauri state. One
/// synchronous connection in WAL mode is plenty for a single-user desktop
/// app — no connection pool, matching the scale this actually needs.
pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create app data dir: {e}"))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("failed to open database: {e}"))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| format!("failed to set WAL mode: {e}"))?;
        migrations::run(&conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite open");
        migrations::run(&conn).expect("run migrations against in-memory db");
        Self(Mutex::new(conn))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use uuid::Uuid;

    /// Proves the real `Db::open` path (a file on disk, not `:memory:`) does
    /// what "survives an app restart" actually means: data written by one
    /// `Db` instance is still there after that instance is dropped (closing
    /// the connection, as quitting the app would) and a fresh `Db::open` is
    /// made against the same path, exactly as `lib.rs`'s `.setup()` does on
    /// every launch.
    #[test]
    fn a_session_survives_closing_and_reopening_the_database_file() {
        let path = std::env::temp_dir().join(format!("aside-test-{}.sqlite3", Uuid::new_v4()));

        let session = {
            let db = Db::open(&path).expect("first open");
            let conn = db.0.lock().unwrap();
            let session = Session::new("Survives restart", 1_000);
            sessions::create(&conn, &session).unwrap();
            session
            // `db` (and its Connection) drops here — simulates the app quitting.
        };

        let reloaded = {
            let db = Db::open(&path).expect("second open");
            let conn = db.0.lock().unwrap();
            sessions::get(&conn, session.id).unwrap()
        };

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));

        assert_eq!(
            reloaded.expect("session should have persisted").title,
            "Survives restart"
        );
    }
}
