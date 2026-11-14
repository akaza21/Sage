//! Per-chat access choices, recorded on the session they were made in; a
//! session that replaces it in the same chat on the same harness takes the
//! choice on (`commands/acp/session_access/lineage.rs`). A binding's default
//! (Settings) covers every
//! chat without a choice, and can never be Full access, so a new chat or a
//! fork always starts at the default.
//!
//! The permissive modes (`SessionAccessMode::needs_confirmation`) take two
//! steps: `prepare_chat_access` issues a single-use token bound to the
//! session and mode that expires, and `grant_chat_access` spends it. A
//! stale or replayed command therefore fails instead of skipping the
//! confirm. Any other choice, which is never more permissive than asking for
//! commands, is set directly and replaces a permissive one.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::{AppError, AppResult};

use super::SettingsStore;
use crate::acp_protocol::access_mode::SessionAccessMode;

/// Chats whose choice is kept. Past it the oldest choice goes first; losing
/// one only returns that chat to its binding's default.
pub const MAX_CHAT_ACCESS_CHOICES: usize = 256;
/// How long a confirmation token stays valid after the dialog opens.
pub const ACCESS_CONFIRMATION_MILLIS: i64 = 120_000;
/// Open confirmations kept at once; one per session.
const MAX_PENDING_CONFIRMATIONS: usize = 8;
const MAX_SESSION_ID_BYTES: usize = 256;
const MODES: &str = "'readOnly','ask','workspaceWrite','acceptEdits','plan','auto','fullAccess'";

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS chat_access_confirmations (
           session_id TEXT PRIMARY KEY CHECK (length(session_id) BETWEEN 1 AND 256),
           mode TEXT NOT NULL CHECK (mode IN ({MODES})),
           token TEXT NOT NULL CHECK (length(token) = 36),
           expires_at INTEGER NOT NULL
         ) STRICT;
         CREATE TABLE IF NOT EXISTS chat_access_choices (
           session_id TEXT PRIMARY KEY CHECK (length(session_id) BETWEEN 1 AND 256),
           mode TEXT NOT NULL,
           chosen_at INTEGER NOT NULL,
           sequence INTEGER NOT NULL
         ) STRICT;"
    ))?;
    Ok(())
}

fn validate_session(session_id: &str) -> AppResult<()> {
    if session_id.is_empty() || session_id.len() > MAX_SESSION_ID_BYTES {
        return Err(AppError::InvalidRequest("Session ID is invalid.".into()));
    }
    Ok(())
}

fn mode_text(mode: SessionAccessMode) -> String {
    match serde_json::to_value(mode) {
        Ok(serde_json::Value::String(text)) => text,
        _ => unreachable!("access modes serialize as strings"),
    }
}

/// A stored mode this build does not know (written by a newer build) reads
/// as no choice, so the chat falls back to its binding's default, the least
/// permissive answer this build can give.
fn read_mode(session_id: &str, text: &str) -> Option<SessionAccessMode> {
    let mode = serde_json::from_value(serde_json::Value::String(text.to_owned())).ok();
    if mode.is_none() {
        tracing::warn!(
            session_id,
            "Unknown stored chat access mode; using the default"
        );
    }
    mode
}

fn record(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    mode: SessionAccessMode,
    now: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO chat_access_choices(session_id,mode,chosen_at,sequence)
         VALUES (?1,?2,?3,(SELECT COALESCE(MAX(sequence),0)+1 FROM chat_access_choices))
         ON CONFLICT(session_id) DO UPDATE SET
           mode=excluded.mode, chosen_at=excluded.chosen_at, sequence=excluded.sequence",
        params![session_id, mode_text(mode), now],
    )?;
    transaction.execute(
        "DELETE FROM chat_access_choices WHERE session_id NOT IN (
           SELECT session_id FROM chat_access_choices ORDER BY sequence DESC LIMIT ?1)",
        [MAX_CHAT_ACCESS_CHOICES as i64],
    )?;
    Ok(())
}

impl SettingsStore {
    /// Sets a chat's mode when it needs no confirmation. This is also how a
    /// chat leaves a permissive mode: the new choice replaces it.
    pub fn set_chat_access(
        &self,
        session_id: &str,
        mode: SessionAccessMode,
        now: i64,
    ) -> AppResult<()> {
        validate_session(session_id)?;
        if mode.needs_confirmation() {
            return Err(AppError::InvalidRequest(
                "This access mode needs a confirmation.".into(),
            ));
        }
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE session_id=?1",
            [session_id],
        )?;
        record(&transaction, session_id, mode, now)?;
        transaction.commit()?;
        Ok(())
    }

    /// Issues the token the confirmation of `mode` must return. A new token
    /// replaces the session's previous one.
    pub fn prepare_chat_access(
        &self,
        session_id: &str,
        mode: SessionAccessMode,
        now: i64,
    ) -> AppResult<String> {
        validate_session(session_id)?;
        if !mode.needs_confirmation() {
            return Err(AppError::InvalidRequest(
                "This access mode needs no confirmation.".into(),
            ));
        }
        let token = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE expires_at <= ?1",
            [now],
        )?;
        transaction.execute(
            "INSERT INTO chat_access_confirmations(session_id,mode,token,expires_at)
             VALUES (?1,?2,?3,?4)
             ON CONFLICT(session_id) DO UPDATE SET
               mode=excluded.mode, token=excluded.token, expires_at=excluded.expires_at",
            params![
                session_id,
                mode_text(mode),
                token,
                now + ACCESS_CONFIRMATION_MILLIS
            ],
        )?;
        transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE session_id NOT IN (
               SELECT session_id FROM chat_access_confirmations
               ORDER BY expires_at DESC LIMIT ?1)",
            [MAX_PENDING_CONFIRMATIONS as i64],
        )?;
        transaction.commit()?;
        Ok(token)
    }

    /// Whether the session holds an unexpired token for `mode`, without
    /// spending it: checked before the person is asked, so a dialog never
    /// opens for a confirmation that could not be granted.
    pub fn chat_access_pending(
        &self,
        session_id: &str,
        mode: SessionAccessMode,
        token: &str,
        now: i64,
    ) -> AppResult<bool> {
        validate_session(session_id)?;
        Ok(self.lock().query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_access_confirmations
             WHERE session_id=?1 AND mode=?2 AND token=?3 AND expires_at > ?4)",
            params![session_id, mode_text(mode), token, now],
            |row| row.get::<_, bool>(0),
        )? && mode.needs_confirmation())
    }

    /// Spends the session's token and records the confirmed mode. The token
    /// works once, only for its session and mode, and only before it expires.
    pub fn grant_chat_access(
        &self,
        session_id: &str,
        mode: SessionAccessMode,
        token: &str,
        now: i64,
    ) -> AppResult<()> {
        validate_session(session_id)?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE expires_at <= ?1",
            [now],
        )?;
        let spent = transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE session_id=?1 AND mode=?2 AND token=?3",
            params![session_id, mode_text(mode), token],
        )?;
        if spent != 1 || !mode.needs_confirmation() {
            return Err(AppError::InvalidRequest(
                "This confirmation is missing, used or expired. Confirm again.".into(),
            ));
        }
        record(&transaction, session_id, mode, now)?;
        transaction.commit()?;
        Ok(())
    }

    /// Returns the chat to its binding's default. Returns whether it had a
    /// choice; calling it again changes nothing.
    pub fn clear_chat_access(&self, session_id: &str) -> AppResult<bool> {
        validate_session(session_id)?;
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM chat_access_confirmations WHERE session_id=?1",
            [session_id],
        )?;
        let cleared = transaction.execute(
            "DELETE FROM chat_access_choices WHERE session_id=?1",
            [session_id],
        )? == 1;
        transaction.commit()?;
        Ok(cleared)
    }

    /// The chat's own mode, if it has one.
    pub fn chat_access(&self, session_id: &str) -> AppResult<Option<SessionAccessMode>> {
        validate_session(session_id)?;
        let text: Option<String> = self
            .lock()
            .query_row(
                "SELECT mode FROM chat_access_choices WHERE session_id=?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(text.and_then(|text| read_mode(session_id, &text)))
    }

    /// Forgets the choices of sessions `exists` no longer knows.
    pub(crate) fn retain_chat_access(
        &self,
        exists: impl Fn(&str) -> AppResult<bool>,
    ) -> AppResult<usize> {
        let sessions: Vec<String> = {
            let connection = self.lock();
            let mut statement = connection.prepare(
                "SELECT session_id FROM chat_access_choices
                 UNION SELECT session_id FROM chat_access_confirmations",
            )?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        };
        let mut forgotten = 0;
        for session in sessions {
            if !exists(&session)? && self.clear_chat_access(&session)? {
                forgotten += 1;
            }
        }
        Ok(forgotten)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Drops the access choices of chat sessions that no longer exist: after a
/// chat, session or project is deleted, and at startup for anything deleted
/// while Sage was not running. Every delete path calls this once its own
/// delete has committed.
pub fn forget_removed_chat_access(
    database: &crate::persistence::Database,
    settings: &SettingsStore,
) {
    let result = settings.retain_chat_access(|session| {
        Ok(database.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id=?1)",
            [session],
            |row| row.get::<_, bool>(0),
        )?)
    });
    if let Err(error) = result {
        tracing::warn!(?error, "Chat access cleanup will retry later");
    }
}

#[cfg(test)]
#[path = "chat_access_tests.rs"]
mod tests;
