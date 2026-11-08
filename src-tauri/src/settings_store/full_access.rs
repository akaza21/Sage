//! Full access grants. Full access applies to one chat session, never to a
//! binding's default, so a new chat (and a fork) always starts without it.
//! Entering it takes two steps: `prepare_full_access` issues a single-use
//! confirmation token that expires, and `grant_full_access` spends it. A
//! stale or replayed grant therefore fails instead of skipping the confirm.

use rusqlite::{Connection, OptionalExtension, params};

use super::SettingsStore;
use crate::error::{AppError, AppResult};

/// Granted sessions kept. Past it the oldest grant goes first; losing one
/// only returns that session to its binding's default, never to more access.
pub const MAX_FULL_ACCESS_SESSIONS: usize = 64;
/// How long a confirmation token stays valid after the dialog opens.
pub const FULL_ACCESS_CONFIRMATION_MILLIS: i64 = 120_000;
/// Open confirmations kept at once; one per session.
const MAX_PENDING_CONFIRMATIONS: usize = 8;
const MAX_SESSION_ID_BYTES: usize = 256;

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS full_access_confirmations (
           session_id TEXT PRIMARY KEY CHECK (length(session_id) BETWEEN 1 AND 256),
           token TEXT NOT NULL CHECK (length(token) = 36),
           expires_at INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS session_full_access (
           session_id TEXT PRIMARY KEY CHECK (length(session_id) BETWEEN 1 AND 256),
           granted_at INTEGER NOT NULL,
           sequence INTEGER NOT NULL
         ) STRICT;",
    )?;
    Ok(())
}

fn validate_session(session_id: &str) -> AppResult<()> {
    if session_id.is_empty() || session_id.len() > MAX_SESSION_ID_BYTES {
        return Err(AppError::InvalidRequest("Session ID is invalid.".into()));
    }
    Ok(())
}

impl SettingsStore {
    /// Issues the confirmation token the Full access dialog must return. A
    /// new token replaces the session's previous one.
    pub fn prepare_full_access(&self, session_id: &str, now: i64) -> AppResult<String> {
        validate_session(session_id)?;
        let token = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM full_access_confirmations WHERE expires_at < ?1",
            [now],
        )?;
        transaction.execute(
            "INSERT INTO full_access_confirmations(session_id,token,expires_at)
             VALUES (?1,?2,?3)
             ON CONFLICT(session_id) DO UPDATE SET
               token=excluded.token, expires_at=excluded.expires_at",
            params![session_id, token, now + FULL_ACCESS_CONFIRMATION_MILLIS],
        )?;
        transaction.execute(
            "DELETE FROM full_access_confirmations WHERE session_id NOT IN (
               SELECT session_id FROM full_access_confirmations
               ORDER BY expires_at DESC LIMIT ?1)",
            [MAX_PENDING_CONFIRMATIONS as i64],
        )?;
        transaction.commit()?;
        Ok(token)
    }

    /// Spends the session's confirmation token and records the grant. The
    /// token works once, only for its session and only before it expires.
    pub fn grant_full_access(&self, session_id: &str, token: &str, now: i64) -> AppResult<()> {
        validate_session(session_id)?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM full_access_confirmations WHERE expires_at < ?1",
            [now],
        )?;
        let spent = transaction.execute(
            "DELETE FROM full_access_confirmations WHERE session_id=?1 AND token=?2",
            params![session_id, token],
        )?;
        if spent != 1 {
            return Err(AppError::InvalidRequest(
                "This Full access confirmation is missing, used or expired. Confirm again.".into(),
            ));
        }
        transaction.execute(
            "INSERT INTO session_full_access(session_id,granted_at,sequence)
             VALUES (?1,?2,(SELECT COALESCE(MAX(sequence),0)+1 FROM session_full_access))
             ON CONFLICT(session_id) DO UPDATE SET
               granted_at=excluded.granted_at, sequence=excluded.sequence",
            params![session_id, now],
        )?;
        transaction.execute(
            "DELETE FROM session_full_access WHERE session_id NOT IN (
               SELECT session_id FROM session_full_access
               ORDER BY sequence DESC LIMIT ?1)",
            [MAX_FULL_ACCESS_SESSIONS as i64],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Ends the session's Full access. Returns whether it had one.
    pub fn revoke_full_access(&self, session_id: &str) -> AppResult<bool> {
        validate_session(session_id)?;
        let connection = self.lock();
        connection.execute(
            "DELETE FROM full_access_confirmations WHERE session_id=?1",
            [session_id],
        )?;
        Ok(connection.execute(
            "DELETE FROM session_full_access WHERE session_id=?1",
            [session_id],
        )? == 1)
    }

    pub fn has_full_access(&self, session_id: &str) -> AppResult<bool> {
        validate_session(session_id)?;
        Ok(self
            .lock()
            .query_row(
                "SELECT 1 FROM session_full_access WHERE session_id=?1",
                [session_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Granted sessions, most recent first.
    pub fn full_access_sessions(&self) -> AppResult<Vec<String>> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT session_id FROM session_full_access ORDER BY sequence DESC LIMIT ?1",
        )?;
        let sessions = statement
            .query_map([MAX_FULL_ACCESS_SESSIONS as i64], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(sessions)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_confirmed_grant_persists_for_its_session_only() {
        let directory = std::env::temp_dir().join(format!("sage-full-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.sqlite");
        let store = SettingsStore::open(&path).unwrap();
        let token = store.prepare_full_access("session-a", 1_000).unwrap();
        store.grant_full_access("session-a", &token, 2_000).unwrap();
        drop(store);
        let store = SettingsStore::open(&path).unwrap();
        assert!(store.has_full_access("session-a").unwrap());
        assert!(!store.has_full_access("session-b").unwrap());
        assert_eq!(store.full_access_sessions().unwrap(), vec!["session-a"]);
        assert!(store.revoke_full_access("session-a").unwrap());
        assert!(!store.revoke_full_access("session-a").unwrap());
        assert!(!store.has_full_access("session-a").unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_replayed_stale_or_foreign_token_never_grants() {
        let store = SettingsStore::in_memory().unwrap();
        // No confirmation at all.
        assert!(store.grant_full_access("session-a", "x", 1).is_err());
        let token = store.prepare_full_access("session-a", 1_000).unwrap();
        // Another session cannot spend it.
        assert!(store.grant_full_access("session-b", &token, 1_001).is_err());
        // It expires.
        assert!(
            store
                .grant_full_access("session-a", &token, 1_001 + FULL_ACCESS_CONFIRMATION_MILLIS)
                .is_err()
        );
        assert!(!store.has_full_access("session-a").unwrap());
        // A newer dialog replaces an older token.
        let older = store.prepare_full_access("session-a", 2_000).unwrap();
        let newer = store.prepare_full_access("session-a", 2_001).unwrap();
        assert!(store.grant_full_access("session-a", &older, 2_002).is_err());
        store.grant_full_access("session-a", &newer, 2_002).unwrap();
        // A replay of the spent token fails even after a revoke.
        store.revoke_full_access("session-a").unwrap();
        assert!(store.grant_full_access("session-a", &newer, 2_003).is_err());
        assert!(!store.has_full_access("session-a").unwrap());
    }

    #[test]
    fn grants_and_confirmations_stay_bounded() {
        let store = SettingsStore::in_memory().unwrap();
        for index in 0..(MAX_FULL_ACCESS_SESSIONS + 3) {
            let session = format!("session-{index}");
            let token = store.prepare_full_access(&session, index as i64).unwrap();
            store
                .grant_full_access(&session, &token, index as i64)
                .unwrap();
        }
        let sessions = store.full_access_sessions().unwrap();
        assert_eq!(sessions.len(), MAX_FULL_ACCESS_SESSIONS);
        assert_eq!(
            sessions[0],
            format!("session-{}", MAX_FULL_ACCESS_SESSIONS + 2)
        );
        assert!(!store.has_full_access("session-0").unwrap());
        for index in 0..(MAX_PENDING_CONFIRMATIONS + 4) {
            store
                .prepare_full_access(&format!("pending-{index}"), 10_000 + index as i64)
                .unwrap();
        }
        let pending: i64 = store
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM full_access_confirmations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending as usize, MAX_PENDING_CONFIRMATIONS);
        assert!(store.prepare_full_access("", 1).is_err());
    }
}
