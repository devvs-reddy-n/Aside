//! Plain key/value settings — chosen STT engine, cloud API keys.
//!
//! Stored as plaintext, matching `docs/decisions.md`'s deliberate scope
//! decision: no OS keychain, no encryption at rest, since this app is never
//! published. Ordinary correctness (parameterized SQL, no string
//! interpolation) still applies — that costs nothing extra.

use rusqlite::{params, Connection, OptionalExtension};

pub fn get(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("failed to read setting {key:?}: {e}"))
}

/// Upsert — settings have no history, only a current value.
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(|e| format!("failed to save setting {key:?}: {e}"))?;
    Ok(())
}

/// Removes a setting entirely, restoring the "nothing chosen" (`None`) state
/// `get` reports for a key that was never set. First real caller: the audio
/// device pickers, where picking "system default" back after pinning a
/// specific device must clear the pin rather than write back today's default
/// as if it were a permanent choice. A no-op, not an error, on a key that was
/// never set — matches `set`'s own idempotent upsert shape.
pub fn delete(conn: &Connection, key: &str) -> Result<(), String> {
    conn.execute("DELETE FROM settings WHERE key = ?1", params![key])
        .map_err(|e| format!("failed to delete setting {key:?}: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Db;

    #[test]
    fn missing_key_returns_none() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        assert_eq!(get(&conn, "stt_engine").unwrap(), None);
    }

    #[test]
    fn set_then_get_round_trips() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        set(&conn, "stt_engine", "parakeet_local").unwrap();
        assert_eq!(
            get(&conn, "stt_engine").unwrap(),
            Some("parakeet_local".to_string())
        );
    }

    #[test]
    fn setting_the_same_key_again_overwrites_rather_than_erroring() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        set(&conn, "stt_engine", "parakeet_local").unwrap();
        set(&conn, "stt_engine", "speechmatics").unwrap();
        assert_eq!(
            get(&conn, "stt_engine").unwrap(),
            Some("speechmatics".to_string())
        );
    }

    #[test]
    fn different_keys_do_not_collide() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        set(&conn, "llm_api_key_openai", "sk_abc").unwrap();
        set(&conn, "stt_api_key_speechmatics", "sm_xyz").unwrap();
        assert_eq!(
            get(&conn, "llm_api_key_openai").unwrap(),
            Some("sk_abc".to_string())
        );
        assert_eq!(
            get(&conn, "stt_api_key_speechmatics").unwrap(),
            Some("sm_xyz".to_string())
        );
    }

    #[test]
    fn delete_removes_a_previously_set_value() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        set(&conn, "mic_device_id", "abc-123").unwrap();
        delete(&conn, "mic_device_id").unwrap();
        assert_eq!(get(&conn, "mic_device_id").unwrap(), None);
    }

    #[test]
    fn delete_on_a_key_that_was_never_set_is_a_harmless_no_op() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        delete(&conn, "mic_device_id").unwrap();
        assert_eq!(get(&conn, "mic_device_id").unwrap(), None);
    }

    #[test]
    fn delete_only_removes_the_named_key() {
        let db = Db::open_in_memory();
        let conn = db.0.lock().unwrap();
        set(&conn, "mic_device_id", "abc-123").unwrap();
        set(&conn, "system_device_id", "xyz-789").unwrap();
        delete(&conn, "mic_device_id").unwrap();
        assert_eq!(get(&conn, "mic_device_id").unwrap(), None);
        assert_eq!(
            get(&conn, "system_device_id").unwrap(),
            Some("xyz-789".to_string())
        );
    }
}
