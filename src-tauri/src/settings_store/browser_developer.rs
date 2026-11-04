//! Browser developer mode (ADR-0009b): the Settings switch, off by default,
//! and the bounded audit log of every raw CDP call an Agent made or tried.
//! Grants are never stored here; they live in memory on the attachment.

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::{
    acp::browser::developer::{CdpAuditEntry, CdpAuditOutcome},
    error::AppResult,
};

/// The newest rows the audit log keeps.
#[cfg_attr(not(windows), allow(dead_code))]
pub const CDP_AUDIT_ROWS: i64 = 5_000;
#[cfg_attr(not(windows), allow(dead_code))]
const OMITTED: &str = "[omitted]";

const AUDIT_TABLE: &str = "CREATE TABLE IF NOT EXISTS browser_cdp_audit (
           id INTEGER PRIMARY KEY,
           at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
           session_id TEXT NOT NULL CHECK (length(session_id) BETWEEN 1 AND 128),
           tab_id INTEGER NOT NULL CHECK (tab_id >= 0),
           origin TEXT NOT NULL CHECK (length(origin) <= 2048),
           method TEXT NOT NULL CHECK (length(method) BETWEEN 1 AND 128),
           params_preview TEXT NOT NULL CHECK (length(CAST(params_preview AS BLOB)) <= 512),
           params_bytes INTEGER NOT NULL CHECK (params_bytes >= 0),
           result_bytes INTEGER CHECK (result_bytes IS NULL OR result_bytes >= 0),
           outcome TEXT NOT NULL CHECK (outcome IN
             ('granted', 'revoked', 'sent', 'ok', 'denied', 'blocked', 'capacity', 'unavailable', 'outcomeUnknown'))
         ) STRICT;";

const AUDIT_SCHEMA_VERSION: i64 = 3;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_developer_mode (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
         ) STRICT;",
    )?;
    // The audit table's schema version, kept in its own row: the settings
    // database has no shared `user_version` scheme, and claiming the whole
    // file's pragma for one table would collide with other settings
    // migrations. Version 1 (dev builds only) lacked the `sent` outcome;
    // version 2 added sent; version 3 adds grant events and omits untrusted text.
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_cdp_audit_schema (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           version INTEGER NOT NULL CHECK (version >= 1)
         ) STRICT;",
    )?;
    let recorded: Option<i64> = connection
        .query_row(
            "SELECT version FROM browser_cdp_audit_schema WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'browser_cdp_audit')",
        [],
        |row| row.get(0),
    )?;
    // A table with no recorded version predates versioning: version 1.
    let version = recorded.unwrap_or(if table_exists { 1 } else { 0 });
    if version == 1 || version == 2 {
        rebuild_audit(connection)?;
    }
    connection.execute_batch(AUDIT_TABLE)?;
    connection.execute(
        "INSERT INTO browser_cdp_audit_schema(singleton, version) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET version = excluded.version",
        params![AUDIT_SCHEMA_VERSION],
    )?;
    Ok(())
}

/// Store only trusted metadata. Arbitrary code, values, unknown method names
/// and caller-supplied session identifiers may contain unrecognizable secrets.
fn session_fingerprint(session: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(session.as_bytes()))
}

fn safe_origin(origin: &str) -> String {
    url::Url::parse(origin).ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.origin().ascii_serialization())
        .filter(|origin| origin.len() <= 2048)
        .unwrap_or_default()
}

fn safe_method(method: &str, grant_event: bool) -> &str {
    if grant_event { return "Grant"; }
    if crate::acp::browser::developer::allowed_methods().contains(&method) { method }
    else { "[unrecognized]" }
}

/// Stream at most the newest 5,000 legacy rows through the same privacy
/// boundary as new writes, inside the caller's migration transaction.
fn rebuild_audit(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(&format!("ALTER TABLE browser_cdp_audit RENAME TO browser_cdp_audit_old; {AUDIT_TABLE}"))?;
    let mut statement = connection.prepare(
        "SELECT id, at_ms, session_id, tab_id, origin, method, params_bytes, result_bytes, outcome
         FROM browser_cdp_audit_old ORDER BY id DESC LIMIT ?1"
    )?;
    let mut rows = statement.query(params![CDP_AUDIT_ROWS])?;
    while let Some(row) = rows.next()? {
        let session: String = row.get(2)?;
        let origin: String = row.get(4)?;
        let method: String = row.get(5)?;
        connection.execute(
            "INSERT INTO browser_cdp_audit(id, at_ms, session_id, tab_id, origin, method,
              params_preview, params_bytes, result_bytes, outcome) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, session_fingerprint(&session),
                row.get::<_, i64>(3)?, safe_origin(&origin), safe_method(&method, false), OMITTED,
                row.get::<_, i64>(6)?, row.get::<_, Option<i64>>(7)?, row.get::<_, String>(8)?],
        )?;
    }
    drop(rows);
    drop(statement);
    connection.execute_batch("DROP TABLE browser_cdp_audit_old;")?;
    Ok(())
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
                session_fingerprint(&entry.session_id),
                i64::try_from(entry.tab_id).unwrap_or(i64::MAX),
                safe_origin(&entry.origin),
                safe_method(&entry.method, matches!(entry.outcome, CdpAuditOutcome::Granted | CdpAuditOutcome::Revoked)),
                OMITTED,
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
    fn a_schema_1_audit_table_is_rebuilt_to_accept_sent_rows_and_keeps_its_rows() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(&AUDIT_TABLE.replace("'sent', ", ""))
            .unwrap();
        connection
            .execute(
                "INSERT INTO browser_cdp_audit(at_ms, session_id, tab_id, origin, method,
                   params_preview, params_bytes, outcome)
                 VALUES (1, 's', 7, 'https://example.com', 'Runtime.evaluate', '{}', 2, 'ok')",
                [],
            )
            .unwrap();
        migrate(&connection).unwrap();
        migrate(&connection).unwrap();
        let version: i64 = connection
            .query_row("SELECT version FROM browser_cdp_audit_schema", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, AUDIT_SCHEMA_VERSION);
        connection
            .execute(
                "INSERT INTO browser_cdp_audit(at_ms, session_id, tab_id, origin, method,
                   params_preview, params_bytes, outcome)
                 VALUES (2, 's', 7, 'https://example.com', 'Runtime.evaluate', '{}', 2, 'sent')",
                [],
            )
            .unwrap();
        let rows: i64 = connection
            .query_row("SELECT count(*) FROM browser_cdp_audit", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2);
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
        assert_eq!(preview, "[omitted]");
        assert!(!preview.contains("s3cr3t-session-value"), "{preview}");
    }

    #[test]
    fn audit_never_persists_untrusted_text_or_url_credentials() {
        let store = SettingsStore::in_memory().unwrap();
        let mut attempt = entry(r#"{"expression":"window.x='plain-secret'","unknown-secret":"value"}"#.into());
        attempt.origin = "https://user:password@example.com/path?token=secret#secret".into();
        attempt.method = "secret-in-invalid-method".into();
        attempt.session_id = "secret-in-session".into();
        attempt.tab_id = 0;
        attempt.outcome = CdpAuditOutcome::Denied;
        store.record_cdp_audit(&attempt, 1).unwrap();
        let (session, origin, method, preview): (String, String, String, String) = store.0.lock().unwrap().query_row(
            "SELECT session_id, origin, method, params_preview FROM browser_cdp_audit", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).unwrap();
        assert_eq!(origin, "https://example.com");
        assert_eq!(method, "[unrecognized]");
        assert_eq!(preview, "[omitted]");
        assert!(!session.contains("secret"));
        assert_eq!(session.len(), 64);
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

    #[test]
    fn legacy_audit_rows_are_scrubbed_during_migration() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(AUDIT_TABLE).unwrap();
        connection.execute("INSERT INTO browser_cdp_audit(at_ms,session_id,tab_id,origin,method,params_preview,params_bytes,outcome) VALUES (1,'secret-session',7,'https://user:password@example.com/path?token=secret','secret-method','plain-secret',12,'denied')", []).unwrap();
        migrate(&connection).unwrap();
        let row: (String, String, String, String, String) = connection.query_row("SELECT session_id,origin,method,params_preview,outcome FROM browser_cdp_audit", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).unwrap();
        assert_eq!(row.0, session_fingerprint("secret-session"));
        assert_eq!(row.1, "https://example.com");
        assert_eq!(row.2, "[unrecognized]");
        assert_eq!(row.3, "[omitted]");
        assert_eq!(row.4, "denied");
    }
}
