//! Durable last-confirmed session config choices (schema v22).
//!
//! The live overlay is never journaled; this table keeps only the value the
//! harness last confirmed for each mode or option, so a resumed runtime can
//! reapply it. Rows are rewritten in place and capped per session.
use rusqlite::{Connection, params};

use super::{ConfigId, ConfigMutation, ConfigValue};
use crate::acp::{
    contract::SageAgentSessionId,
    limits::{SESSION_CONFIG_CHOICE_BYTES, SESSION_CONFIG_CHOICES_PER_SESSION},
};

const MODE_KIND: &str = "mode";
const OPTION_KIND: &str = "option";
/// The mode has no config ID of its own; its row uses this fixed key.
const MODE_KEY: &str = "mode";

fn columns(choice: &ConfigMutation) -> (&'static str, &str, ConfigValue) {
    match choice {
        ConfigMutation::Mode(mode) => (
            MODE_KIND,
            MODE_KEY,
            ConfigValue::Select {
                value: mode.clone(),
            },
        ),
        ConfigMutation::Option { config_id, value } => {
            (OPTION_KIND, config_id.as_str(), value.clone())
        }
    }
}

fn from_columns(kind: &str, config_id: String, value_json: &str) -> Option<ConfigMutation> {
    let value: ConfigValue = serde_json::from_str(value_json).ok()?;
    Some(match (kind, value) {
        (MODE_KIND, ConfigValue::Select { value }) => ConfigMutation::Mode(value),
        (OPTION_KIND, value) => ConfigMutation::Option {
            config_id: ConfigId::new("configId", config_id).ok()?,
            value,
        },
        _ => return None,
    })
}

/// Stores `choice` as the session's confirmed value for its mode or option.
/// A new option beyond the per-session cap, or an oversized value, is not
/// stored. Returns whether a row was written.
pub(crate) fn record(
    connection: &Connection,
    session: &SageAgentSessionId,
    choice: &ConfigMutation,
    now_millis: i64,
) -> rusqlite::Result<bool> {
    let (kind, config_id, value) = columns(choice);
    let value_json = value.wire().to_string();
    if value_json.len() > SESSION_CONFIG_CHOICE_BYTES {
        return Ok(false);
    }
    let written = connection.execute(
        "INSERT INTO acp_session_config_choices(session_id,kind,config_id,value_json,updated_at)
         SELECT ?1,?2,?3,?4,?5
         WHERE (SELECT COUNT(*) FROM acp_session_config_choices
                WHERE session_id=?1 AND NOT (kind=?2 AND config_id=?3)) < ?6
         ON CONFLICT(session_id,kind,config_id)
         DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at",
        params![
            session.as_str(),
            kind,
            config_id,
            value_json,
            now_millis,
            SESSION_CONFIG_CHOICES_PER_SESSION as i64
        ],
    )?;
    Ok(written > 0)
}

/// Removes the stored value of the mode or option `choice` names.
pub(crate) fn forget(
    connection: &Connection,
    session: &SageAgentSessionId,
    choice: &ConfigMutation,
) -> rusqlite::Result<()> {
    let (kind, config_id, _) = columns(choice);
    connection.execute(
        "DELETE FROM acp_session_config_choices WHERE session_id=?1 AND kind=?2 AND config_id=?3",
        params![session.as_str(), kind, config_id],
    )?;
    Ok(())
}

/// Gives session `to` the confirmed choices of session `from` (a fork
/// continues its parent's session). Runs in the caller's transaction.
pub(crate) fn copy(
    connection: &Connection,
    from: &SageAgentSessionId,
    to: &SageAgentSessionId,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO acp_session_config_choices(session_id,kind,config_id,value_json,updated_at)
         SELECT ?2,kind,config_id,value_json,updated_at FROM acp_session_config_choices
         WHERE session_id=?1 ORDER BY updated_at LIMIT ?3",
        params![
            from.as_str(),
            to.as_str(),
            SESSION_CONFIG_CHOICES_PER_SESSION as i64
        ],
    )?;
    Ok(())
}

/// The session's stored choices in the order they were confirmed. Restore
/// reorders them by what the live session says each option is. Rows this
/// build cannot decode are skipped.
pub(crate) fn load(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> rusqlite::Result<Vec<ConfigMutation>> {
    let mut statement = connection.prepare(
        "SELECT kind,config_id,value_json FROM acp_session_config_choices
         WHERE session_id=?1 ORDER BY updated_at,kind,config_id LIMIT ?2",
    )?;
    let rows = statement.query_map(
        params![session.as_str(), SESSION_CONFIG_CHOICES_PER_SESSION as i64],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut choices = Vec::new();
    for row in rows {
        let (kind, config_id, value_json) = row?;
        if let Some(choice) = from_columns(&kind, config_id, &value_json) {
            choices.push(choice);
        }
    }
    Ok(choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_keeps_one_row_per_option_up_to_the_cap() {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let connection = database.connection();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1);").unwrap();
        connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'{}','created','cold',1,1)", [&"a".repeat(64)]).unwrap();
        let session = SageAgentSessionId::parse("session").unwrap();
        let select = |id: &str, value: &str| ConfigMutation::Option {
            config_id: ConfigId::new("configId", id).unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", value).unwrap(),
            },
        };
        for index in 0..SESSION_CONFIG_CHOICES_PER_SESSION {
            assert!(record(&connection, &session, &select(&format!("o{index}"), "a"), 1).unwrap());
        }
        // A new option past the cap is not stored; a known one still updates.
        assert!(!record(&connection, &session, &select("extra", "a"), 2).unwrap());
        assert!(record(&connection, &session, &select("o0", "b"), 3).unwrap());
        let stored = load(&connection, &session).unwrap();
        assert_eq!(stored.len(), SESSION_CONFIG_CHOICES_PER_SESSION);
        assert_eq!(stored.last(), Some(&select("o0", "b")));
        forget(&connection, &session, &select("o0", "b")).unwrap();
        assert!(
            !load(&connection, &session)
                .unwrap()
                .contains(&select("o0", "b"))
        );
    }

    #[test]
    fn a_fork_starts_with_its_parents_choices() {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let connection = database.connection();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('parent','project',0,'Parent',1,1),('child','project',1,'Child',1,1);").unwrap();
        for (id, chat) in [("session", "parent"), ("fork", "child")] {
            connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES(?1,'project',?2,0,'binding',?3,'{}','created','cold',1,1)", params![id, chat, "a".repeat(64)]).unwrap();
        }
        let parent = SageAgentSessionId::parse("session").unwrap();
        let fork = SageAgentSessionId::parse("fork").unwrap();
        let mode = ConfigMutation::Mode(ConfigId::new("modeId", "plan").unwrap());
        let fast = ConfigMutation::Option {
            config_id: ConfigId::new("configId", "fast").unwrap(),
            value: ConfigValue::Boolean { value: true },
        };
        assert!(record(&connection, &parent, &mode, 1).unwrap());
        assert!(record(&connection, &parent, &fast, 2).unwrap());
        copy(&connection, &parent, &fork).unwrap();
        assert_eq!(load(&connection, &fork).unwrap(), [mode.clone(), fast]);
        // The two sessions' choices are separate from then on.
        forget(&connection, &fork, &mode).unwrap();
        assert!(load(&connection, &parent).unwrap().contains(&mode));
    }
}
