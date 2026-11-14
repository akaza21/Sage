//! The live overlay is never journaled; this table keeps only the value the
//! harness last confirmed for each mode or option, so a resumed runtime can
//! reapply it.
use rusqlite::{Connection, OptionalExtension, params};

use sage_acp_protocol::contract::{BoundedVec, SageAgentSessionId};
use sage_acp_protocol::session_config::{
    ConfigId, ConfigKind, ConfigMutation, ConfigValue, SessionConfig,
};
use sage_core::limits::{LIMITS, SESSION_CONFIG_CHOICE_BYTES, SESSION_CONFIG_CHOICES_PER_SESSION};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RestoredSessionConfig {
    pub sage_agent_session_id: SageAgentSessionId,
    pub awaiting: Awaiting,
    #[serde(flatten)]
    pub config: SessionConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Awaiting {
    /// No runtime: the choices are reapplied when the chat resumes.
    Resume,
    /// A live runtime that has not reported its config yet: the choices are
    /// reapplied once it does.
    Report,
}

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

/// Runs in the caller's transaction.
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
    connection.execute(
        "INSERT INTO acp_session_config_snapshots(session_id,config_json,updated_at)
         SELECT ?2,config_json,updated_at FROM acp_session_config_snapshots WHERE session_id=?1",
        params![from.as_str(), to.as_str()],
    )?;
    Ok(())
}

/// A config past the live bound is not stored.
pub(crate) fn save_snapshot(
    connection: &Connection,
    session: &SageAgentSessionId,
    config: &SessionConfig,
    now_millis: i64,
) -> rusqlite::Result<()> {
    let Ok(config_json) = serde_json::to_string(config) else {
        return Ok(());
    };
    if config_json.len() > LIMITS.wire.models_or_config_bytes as usize {
        return Ok(());
    }
    connection.execute(
        "INSERT INTO acp_session_config_snapshots(session_id,config_json,updated_at)
         VALUES(?1,?2,?3)
         ON CONFLICT(session_id) DO UPDATE SET
           config_json=excluded.config_json,updated_at=excluded.updated_at",
        params![session.as_str(), config_json, now_millis],
    )?;
    Ok(())
}

/// A stored choice the
/// snapshot does not offer is left out (the restore falls back for it); with
/// no snapshot, each stored option shows only its own value. `None` when
/// nothing is stored.
pub(crate) fn restored(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> rusqlite::Result<Option<SessionConfig>> {
    let snapshot = connection
        .query_row(
            "SELECT config_json FROM acp_session_config_snapshots WHERE session_id=?1",
            [session.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|json| serde_json::from_str::<SessionConfig>(&json).ok())
        .filter(|config| config.validate().is_ok());
    with_stored(connection, session, snapshot)
}

/// Only the stored choices, each shown with its own value: the smaller view
/// a page shows when the full snapshot does not fit its budget.
pub(crate) fn stored_only(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> rusqlite::Result<Option<SessionConfig>> {
    with_stored(connection, session, None)
}

fn with_stored(
    connection: &Connection,
    session: &SageAgentSessionId,
    snapshot: Option<SessionConfig>,
) -> rusqlite::Result<Option<SessionConfig>> {
    let stored = load(connection, session)?;
    if snapshot.is_none() && stored.is_empty() {
        return Ok(None);
    }
    let mut config = snapshot.unwrap_or_default();
    for choice in &stored {
        apply(&mut config, choice);
    }
    Ok(Some(config))
}

pub(crate) fn get(
    connection: &Connection,
    session: &SageAgentSessionId,
    choice: &ConfigMutation,
) -> rusqlite::Result<Option<ConfigMutation>> {
    let (kind, config_id, _) = columns(choice);
    Ok(connection
        .query_row(
            "SELECT value_json FROM acp_session_config_choices
             WHERE session_id=?1 AND kind=?2 AND config_id=?3",
            params![session.as_str(), kind, config_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|value_json| from_columns(kind, config_id.to_owned(), &value_json)))
}

/// Compare-and-set: while `expected` is still the stored value of its mode
/// or option, stores `new` for it (`None` removes it), settles any notice
/// about it and sets `notice`, all in one transaction. A newer pick stored
/// meanwhile is left as it is. Returns whether `expected` was replaced.
pub(crate) fn replace_if_stored(
    connection: &mut Connection,
    session: &SageAgentSessionId,
    expected: &ConfigMutation,
    new: Option<&ConfigMutation>,
    notice: Option<&super::notices::SessionConfigNotice>,
    now_millis: i64,
) -> rusqlite::Result<bool> {
    let (kind, config_id, value) = columns(expected);
    let expected_json = value.wire().to_string();
    let transaction = connection.transaction()?;
    let replaced = match new.map(columns) {
        Some((new_kind, new_id, new_value)) => {
            if (new_kind, new_id) != (kind, config_id) {
                return Ok(false);
            }
            transaction.execute(
                "UPDATE acp_session_config_choices SET value_json=?5,updated_at=?6
                 WHERE session_id=?1 AND kind=?2 AND config_id=?3 AND value_json=?4",
                params![
                    session.as_str(),
                    kind,
                    config_id,
                    expected_json,
                    new_value.wire().to_string(),
                    now_millis
                ],
            )?
        }
        None => transaction.execute(
            "DELETE FROM acp_session_config_choices
             WHERE session_id=?1 AND kind=?2 AND config_id=?3 AND value_json=?4",
            params![session.as_str(), kind, config_id, expected_json],
        )?,
    } > 0;
    if replaced {
        super::notices::clear(&transaction, session, expected)?;
        if let Some(notice) = notice {
            super::notices::set(&transaction, session, notice, now_millis)?;
        }
    }
    transaction.commit()?;
    Ok(replaced)
}

/// An option `config` lacks is added with the stored value as its only
/// choice, so its label still shows.
fn apply(config: &mut SessionConfig, choice: &ConfigMutation) {
    match choice {
        ConfigMutation::Mode(mode) => {
            let _ = config.set_current_mode(mode.clone());
        }
        ConfigMutation::Option { config_id, value } => {
            let mut options = config.config_options.as_slice().to_vec();
            match options.iter_mut().find(|option| &option.id == config_id) {
                Some(option) => {
                    if config.validate_value(config_id.as_str(), value).is_err() {
                        return;
                    }
                    option.kind = match (&option.kind, value) {
                        (ConfigKind::Select { options, .. }, ConfigValue::Select { value }) => {
                            ConfigKind::Select {
                                current_value: value.clone(),
                                options: options.clone(),
                            }
                        }
                        (ConfigKind::Boolean { .. }, ConfigValue::Boolean { value }) => {
                            ConfigKind::Boolean {
                                current_value: *value,
                            }
                        }
                        _ => return,
                    };
                }
                None => {
                    let ConfigValue::Select { value } = value else {
                        return;
                    };
                    let Ok(name) =
                        sage_acp_protocol::session_config::Label::new("name", config_id.as_str())
                    else {
                        return;
                    };
                    let Ok(choice_name) =
                        sage_acp_protocol::session_config::Label::new("name", value.as_str())
                    else {
                        return;
                    };
                    let Ok(choices) = BoundedVec::new(
                        "options",
                        vec![sage_acp_protocol::session_config::Choice {
                            value: value.clone(),
                            name: choice_name,
                            description: None,
                        }],
                    ) else {
                        return;
                    };
                    options.push(sage_acp_protocol::session_config::ConfigOption {
                        id: config_id.clone(),
                        name,
                        description: None,
                        category: None,
                        default_value: None,
                        kind: ConfigKind::Select {
                            current_value: value.clone(),
                            options: sage_acp_protocol::session_config::Choices::Flat(choices),
                        },
                    });
                }
            }
            let mut candidate = config.clone();
            let Ok(options) =
                sage_acp_protocol::session_config::ConfigOptions::new("configOptions", options)
            else {
                return;
            };
            candidate.config_options = options;
            if candidate.validate().is_ok() {
                *config = candidate;
            }
        }
    }
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
        let mut connection = database.connection();
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
        assert!(!record(&connection, &session, &select("extra", "a"), 2).unwrap());
        assert!(record(&connection, &session, &select("o0", "b"), 3).unwrap());
        let stored = load(&connection, &session).unwrap();
        assert_eq!(stored.len(), SESSION_CONFIG_CHOICES_PER_SESSION);
        assert_eq!(stored.last(), Some(&select("o0", "b")));
        assert!(
            replace_if_stored(&mut connection, &session, &select("o0", "b"), None, None, 4)
                .unwrap()
        );
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
        let mut connection = database.connection();
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
        assert!(replace_if_stored(&mut connection, &fork, &mode, None, None, 3).unwrap());
        assert!(load(&connection, &parent).unwrap().contains(&mode));
    }

    #[test]
    fn a_compare_and_set_leaves_a_newer_value_and_its_notice_alone() {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let mut connection = database.connection();
        connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
            INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1);").unwrap();
        connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'{}','created','cold',1,1)", [&"a".repeat(64)]).unwrap();
        let session = SageAgentSessionId::parse("session").unwrap();
        let model = |value: &str| ConfigMutation::Option {
            config_id: ConfigId::new("configId", "model").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", value).unwrap(),
            },
        };
        let notice = |saved: &str| super::super::notices::SessionConfigNotice {
            config_id: ConfigId::new("configId", "model").unwrap(),
            saved: ConfigId::new("saved", saved).unwrap(),
            current: ConfigId::new("current", "a").unwrap(),
            reason: super::super::notices::NoticeReason::NotOffered,
        };
        record(&connection, &session, &model("b"), 1).unwrap();
        super::super::notices::set(&connection, &session, &notice("old"), 1).unwrap();
        let stale = model("retired");
        assert!(
            !replace_if_stored(
                &mut connection,
                &session,
                &stale,
                Some(&model("a")),
                Some(&notice("retired")),
                2
            )
            .unwrap()
        );
        assert!(!replace_if_stored(&mut connection, &session, &stale, None, None, 2).unwrap());
        assert_eq!(load(&connection, &session).unwrap(), [model("b")]);
        let kept = super::super::notices::load(&connection, &session).unwrap();
        assert_eq!(kept.unwrap().saved.as_str(), "old");
        assert!(
            replace_if_stored(
                &mut connection,
                &session,
                &model("b"),
                Some(&model("a")),
                None,
                3
            )
            .unwrap()
        );
        assert_eq!(load(&connection, &session).unwrap(), [model("a")]);
        assert_eq!(
            super::super::notices::load(&connection, &session).unwrap(),
            None
        );
    }
}
