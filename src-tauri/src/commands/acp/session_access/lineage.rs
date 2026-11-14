//! A chat's access choice belongs to the chat, not to the session it was
//! made in. A chat replaces its session after a failure, an end or a
//! recovery, and the replacement on the same harness runs under the newest
//! choice made in the chat's earlier sessions on that harness. A session's
//! own choice always comes first; another harness's choices never apply,
//! since each harness carries its own modes.

use crate::acp_protocol::access_mode::SessionAccessMode;
use crate::persistence::Database;
use crate::settings_store::SettingsStore;
use sage_core::error::AppResult;
use sage_core::limits::LIMITS;

/// The chat's access choice as `session_id` holds it: its own, else the
/// newest one an earlier session of its chat on its harness held.
pub(crate) fn chat_choice(
    database: &Database,
    settings: &SettingsStore,
    session_id: &str,
) -> AppResult<Option<SessionAccessMode>> {
    if let Some(mode) = settings.chat_access(session_id)? {
        return Ok(Some(mode));
    }
    for earlier in earlier_sessions(database, session_id)? {
        if let Some(mode) = settings.chat_access(&earlier)? {
            return Ok(Some(mode));
        }
    }
    Ok(None)
}

/// The chat's earlier sessions on the same harness, newest first, bounded
/// by the sessions a chat may hold.
fn earlier_sessions(database: &Database, session_id: &str) -> AppResult<Vec<String>> {
    let connection = database.connection();
    let mut statement = connection.prepare(
        "SELECT earlier.id FROM agent_sessions AS this
         JOIN agent_sessions AS earlier
           ON earlier.project_id=this.project_id AND earlier.chat_id=this.chat_id
          AND earlier.binding_id=this.binding_id AND earlier.ordinal<this.ordinal
         WHERE this.id=?1
         ORDER BY earlier.ordinal DESC LIMIT ?2",
    )?;
    let sessions = statement
        .query_map(
            rusqlite::params![session_id, LIMITS.durable.sessions_per_chat as i64],
            |row| row.get(0),
        )?
        .collect::<Result<_, _>>()?;
    Ok(sessions)
}

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
