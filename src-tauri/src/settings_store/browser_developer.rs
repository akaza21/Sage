//! Browser developer mode (ADR-0009b): the Settings switch, off by default,
//! and the bounded audit log of every raw CDP call an Agent made or tried.
//! Grants are never stored here; they live in memory on the attachment.

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::{
    acp::browser::{developer::CdpAuditEntry, redact_console, redact_json},
    error::AppResult,
};

/// The newest rows the audit log keeps.
#[cfg_attr(not(windows), allow(dead_code))]
pub const CDP_AUDIT_ROWS: i64 = 5_000;
#[cfg_attr(not(windows), allow(dead_code))]
const PARAMS_PREVIEW_BYTES: usize = 512;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_developer_mode (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
         ) STRICT;
         CREATE TABLE IF NOT EXISTS browser_cdp_audit (
           id INTEGER PRIMARY KEY,
           at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
           session_id TEXT NOT NULL CHECK (length(session_id) BETWEEN 1 AND 128),
           tab_id INTEGER NOT NULL CHECK (tab_id > 0),
           origin TEXT NOT NULL CHECK (length(origin) <= 2048),
           method TEXT NOT NULL CHECK (length(method) BETWEEN 1 AND 128),
           params_preview TEXT NOT NULL CHECK (length(CAST(params_preview AS BLOB)) <= 512),
           params_bytes INTEGER NOT NULL CHECK (params_bytes >= 0),
           result_bytes INTEGER CHECK (result_bytes IS NULL OR result_bytes >= 0),
           outcome TEXT NOT NULL CHECK (outcome IN
             ('sent', 'ok', 'denied', 'blocked', 'capacity', 'unavailable', 'outcomeUnknown'))
         ) STRICT;",
    )?;
    Ok(())
}

/// The redacted parameters, cut at a character boundary within the cap.
#[cfg_attr(not(windows), allow(dead_code))]
fn preview(params: &str) -> String {
    // Secret-named fields go first and every string is redacted on its
    // own; a cookie's or header's `value` beside its `name` is always
    // dropped. Text that is not JSON is redacted as a whole.
    let mut redacted = match serde_json::from_str::<serde_json::Value>(params) {
        Ok(mut value) => {
            redact_json(&mut value);
            drop_named_values(&mut value);
            value.to_string()
        }
        Err(_) => redact_console(params),
    };
    if redacted.len() > PARAMS_PREVIEW_BYTES {
        let mut end = PARAMS_PREVIEW_BYTES;
        while !redacted.is_char_boundary(end) {
            end -= 1;
        }
        redacted.truncate(end);
    }
    redacted
}

#[cfg_attr(not(windows), allow(dead_code))]
fn drop_named_values(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if map.contains_key("name") && map.contains_key("value") {
                map.insert("value".into(), "[redacted]".into());
            }
            map.values_mut().for_each(drop_named_values);
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(drop_named_values),
        _ => {}
    }
}

impl SettingsStore {
    pub fn browser_developer_mode(&self) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(connection
            .query_row(
                "SELECT enabled FROM browser_developer_mode WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn set_browser_developer_mode(&self, enabled: bool) -> AppResult<bool> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO browser_developer_mode(singleton, enabled) VALUES (1, ?1)
             ON CONFLICT(singleton) DO UPDATE SET enabled = excluded.enabled",
            params![enabled],
        )?;
        Ok(enabled)
    }

    /// Appends one row and drops rows past the newest `CDP_AUDIT_ROWS`, in
    /// one transaction.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn record_cdp_audit(&self, entry: &CdpAuditEntry, at_ms: i64) -> AppResult<()> {
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO browser_cdp_audit(at_ms, session_id, tab_id, origin, method,
               params_preview, params_bytes, result_bytes, outcome)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                at_ms,
                entry.session_id,
                i64::try_from(entry.tab_id).unwrap_or(i64::MAX),
                entry.origin,
                entry.method,
                preview(&entry.params),
                i64::try_from(entry.params.len()).unwrap_or(i64::MAX),
                entry
                    .result_bytes
                    .map(|bytes| i64::try_from(bytes).unwrap_or(i64::MAX)),
                entry.outcome.as_str(),
            ],
        )?;
        transaction.execute(
            "DELETE FROM browser_cdp_audit WHERE id <= (SELECT max(id) FROM browser_cdp_audit) - ?1",
            params![CDP_AUDIT_ROWS],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::browser::developer::CdpAuditOutcome;

    fn entry(params: String) -> CdpAuditEntry {
        CdpAuditEntry {
            session_id: "session-1".into(),
            tab_id: 7,
            origin: "https://example.com".into(),
            method: "Runtime.evaluate".into(),
            params,
            result_bytes: Some(12),
            outcome: CdpAuditOutcome::Ok,
        }
    }

    fn count(store: &SettingsStore) -> i64 {
        store
            .0
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM browser_cdp_audit", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn the_switch_is_off_until_set_and_survives_reopening() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-cdp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let store = SettingsStore::open(&path).unwrap();
        assert!(!store.browser_developer_mode().unwrap());
        assert!(store.set_browser_developer_mode(true).unwrap());
        drop(store);
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.browser_developer_mode().unwrap());
        assert!(!store.set_browser_developer_mode(false).unwrap());
        assert!(!store.browser_developer_mode().unwrap());
        drop(store);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn audit_rows_keep_a_redacted_bounded_preview() {
        let store = SettingsStore::in_memory().unwrap();
        let secret = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let params = format!(
            r#"{{"expression":"fetch('/x',{{headers:{{a:'{secret}'}}}})//{}é"}}"#,
            "x".repeat(600)
        );
        store.record_cdp_audit(&entry(params.clone()), 1).unwrap();
        let (preview, bytes, outcome): (String, i64, String) = store
            .0
            .lock()
            .unwrap()
            .query_row(
                "SELECT params_preview, params_bytes, outcome FROM browser_cdp_audit",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert!(!preview.contains(secret));
        assert!(preview.len() <= 512);
        assert_eq!(bytes, params.len() as i64);
        assert_eq!(outcome, "ok");
    }

    #[test]
    fn a_cookie_value_in_the_parameters_never_reaches_the_preview() {
        let store = SettingsStore::in_memory().unwrap();
        let params =
            r#"{"name":"sid","value":"s3cr3t-session-value","url":"https://example.com/"}"#;
        store.record_cdp_audit(&entry(params.into()), 1).unwrap();
        let preview: String = store
            .0
            .lock()
            .unwrap()
            .query_row("SELECT params_preview FROM browser_cdp_audit", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(preview.contains("https://example.com/"), "{preview}");
        assert!(!preview.contains("s3cr3t-session-value"), "{preview}");
    }

    #[test]
    fn the_audit_log_keeps_only_the_newest_rows() {
        let store = SettingsStore::in_memory().unwrap();
        for at in 0..(CDP_AUDIT_ROWS + 3) {
            store.record_cdp_audit(&entry("{}".into()), at).unwrap();
        }
        assert_eq!(count(&store), CDP_AUDIT_ROWS);
        let oldest: i64 = store
            .0
            .lock()
            .unwrap()
            .query_row("SELECT min(at_ms) FROM browser_cdp_audit", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(oldest, 3);
    }
}
