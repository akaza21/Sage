//! User-defined ACP agents: a display name, an executable and its
//! arguments, and the names of environment variables to pass through.
//! Only variable names are stored, never their values, so a secret stays
//! in the user's environment. The list is closed at four slots; each slot
//! owns a fixed harness identity (`custom-acp-1` ... `custom-acp-4`).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::SettingsStore;
use crate::error::{AppError, AppResult};

pub const MAX_CUSTOM_ACP_AGENTS: usize = 4;
const NAME_CHARS: usize = 64;
const EXECUTABLE_BYTES: usize = 1024;
const MAX_ARGS: usize = 16;
const ARG_BYTES: usize = 512;
const MAX_ENV_NAMES: usize = 16;
const ENV_NAME_BYTES: usize = 128;
const ID_PREFIX: &str = "custom-acp-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomAcpAgent {
    pub id: String,
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
}

/// An agent as the user submits it; `id` is absent for a new agent.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CustomAcpAgentDraft {
    pub id: Option<String>,
    pub name: String,
    pub executable: String,
    pub args: Vec<String>,
    pub env: Vec<String>,
}

/// The 1-based slot a custom agent id names.
pub fn custom_acp_slot(id: &str) -> Option<usize> {
    let slot: usize = id.strip_prefix(ID_PREFIX)?.parse().ok()?;
    (1..=MAX_CUSTOM_ACP_AGENTS).contains(&slot).then_some(slot)
}

pub fn custom_acp_id(slot: usize) -> String {
    format!("{ID_PREFIX}{slot}")
}

pub(super) fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS custom_acp_agents (
           slot INTEGER PRIMARY KEY CHECK (slot BETWEEN 1 AND 4),
           name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 256),
           executable TEXT NOT NULL CHECK (length(executable) BETWEEN 1 AND 1024),
           args_json TEXT NOT NULL CHECK (length(args_json) <= 16384),
           env_json TEXT NOT NULL CHECK (length(env_json) <= 4096)
         ) STRICT;",
    )
}

fn invalid(message: &str) -> AppError {
    AppError::InvalidRequest(format!("custom ACP agent {message}"))
}

/// Checks the shape of a submitted agent. Whether the executable exists is
/// the caller's check, made against the disk when the agent is admitted.
pub fn validate_custom_acp_agent(
    draft: &CustomAcpAgentDraft,
) -> AppResult<(String, String, Vec<String>, Vec<String>)> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > NAME_CHARS || name.chars().any(char::is_control) {
        return Err(invalid("name must be 1 to 64 printable characters"));
    }
    let executable = draft.executable.trim();
    if executable.is_empty()
        || executable.len() > EXECUTABLE_BYTES
        || executable.chars().any(char::is_control)
        || !Path::new(executable).is_absolute()
    {
        return Err(invalid("executable must be an absolute path"));
    }
    if draft.args.len() > MAX_ARGS
        || draft
            .args
            .iter()
            .any(|arg| arg.is_empty() || arg.len() > ARG_BYTES || arg.chars().any(char::is_control))
    {
        return Err(invalid(
            "arguments must be at most 16 non-empty printable values",
        ));
    }
    let mut env = Vec::with_capacity(draft.env.len());
    for name in &draft.env {
        let name = name.trim();
        if !env_name(name)
            || env
                .iter()
                .any(|seen: &String| seen.eq_ignore_ascii_case(name))
        {
            return Err(invalid(
                "environment entries must be distinct variable names, not values",
            ));
        }
        env.push(name.to_owned());
    }
    if env.len() > MAX_ENV_NAMES {
        return Err(invalid("passes through at most 16 variables"));
    }
    Ok((
        name.to_owned(),
        executable.to_owned(),
        draft.args.clone(),
        env,
    ))
}

fn env_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= ENV_NAME_BYTES
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

impl SettingsStore {
    pub fn custom_acp_agents(&self) -> AppResult<Vec<CustomAcpAgent>> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT slot, name, executable, args_json, env_json FROM custom_acp_agents ORDER BY slot LIMIT 4",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut agents = Vec::with_capacity(MAX_CUSTOM_ACP_AGENTS);
        for row in rows {
            let (slot, name, executable, args, env) = row?;
            let draft = CustomAcpAgentDraft {
                id: None,
                name,
                executable,
                args: serde_json::from_str(&args).unwrap_or_default(),
                env: serde_json::from_str(&env).unwrap_or_default(),
            };
            // A row that no longer passes validation is skipped, never launched.
            let Ok((name, executable, args, env)) = validate_custom_acp_agent(&draft) else {
                continue;
            };
            agents.push(CustomAcpAgent {
                id: custom_acp_id(usize::try_from(slot).unwrap_or_default()),
                name,
                executable,
                args,
                env,
            });
        }
        Ok(agents)
    }

    /// Saves `draft` into its slot, or the first free slot for a new agent.
    pub fn save_custom_acp_agent(&self, draft: &CustomAcpAgentDraft) -> AppResult<CustomAcpAgent> {
        let (name, executable, args, env) = validate_custom_acp_agent(draft)?;
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let slot = match draft.id.as_deref() {
            Some(id) => custom_acp_slot(id).ok_or_else(|| invalid("id is unknown"))?,
            None => free_slot(&connection)?
                .ok_or_else(|| invalid("limit reached: remove an agent before adding another"))?,
        };
        connection.execute(
            "INSERT INTO custom_acp_agents(slot, name, executable, args_json, env_json)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(slot) DO UPDATE SET name=excluded.name, executable=excluded.executable,
               args_json=excluded.args_json, env_json=excluded.env_json",
            params![
                slot as i64,
                name,
                executable,
                serde_json::to_string(&args).map_err(|_| invalid("arguments are invalid"))?,
                serde_json::to_string(&env).map_err(|_| invalid("environment is invalid"))?,
            ],
        )?;
        Ok(CustomAcpAgent {
            id: custom_acp_id(slot),
            name,
            executable,
            args,
            env,
        })
    }

    pub fn remove_custom_acp_agent(&self, id: &str) -> AppResult<usize> {
        let slot = custom_acp_slot(id).ok_or_else(|| invalid("id is unknown"))?;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .execute("DELETE FROM custom_acp_agents WHERE slot=?1", [slot as i64])?;
        Ok(slot)
    }
}

fn free_slot(connection: &Connection) -> AppResult<Option<usize>> {
    for slot in 1..=MAX_CUSTOM_ACP_AGENTS {
        let taken = connection
            .query_row(
                "SELECT 1 FROM custom_acp_agents WHERE slot=?1",
                [slot as i64],
                |_| Ok(()),
            )
            .optional()?;
        if taken.is_none() {
            return Ok(Some(slot));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(name: &str) -> CustomAcpAgentDraft {
        CustomAcpAgentDraft {
            id: None,
            name: name.into(),
            executable: if cfg!(windows) {
                r"C:\agents\agent.exe".into()
            } else {
                "/opt/agents/agent".into()
            },
            args: vec!["--acp".into()],
            env: vec!["AGENT_API_KEY".into()],
        }
    }

    #[test]
    fn agents_fill_four_slots_and_survive_reopening() {
        let directory =
            crate::test_paths::temp_dir().join(format!("sage-custom-acp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let store = SettingsStore::open(&path).unwrap();
        for index in 1..=4 {
            let saved = store
                .save_custom_acp_agent(&draft(&format!("Agent {index}")))
                .unwrap();
            assert_eq!(saved.id, format!("custom-acp-{index}"));
        }
        assert!(store.save_custom_acp_agent(&draft("Fifth")).is_err());
        assert_eq!(store.remove_custom_acp_agent("custom-acp-2").unwrap(), 2);
        assert_eq!(
            store.save_custom_acp_agent(&draft("Again")).unwrap().id,
            "custom-acp-2"
        );
        drop(store);
        let reopened = SettingsStore::open(&path)
            .unwrap()
            .custom_acp_agents()
            .unwrap();
        assert_eq!(reopened.len(), 4);
        assert_eq!(reopened[1].name, "Again");
        assert_eq!(reopened[1].env, ["AGENT_API_KEY"]);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn values_are_refused_where_only_names_belong() {
        let store = SettingsStore::in_memory().unwrap();
        for env in [
            vec!["AGENT_API_KEY=secret".to_owned()],
            vec!["1BAD".to_owned()],
            vec!["KEY".to_owned(), "key".to_owned()],
            vec!["K".repeat(129)],
        ] {
            assert!(
                store
                    .save_custom_acp_agent(&CustomAcpAgentDraft {
                        env,
                        ..draft("Agent")
                    })
                    .is_err()
            );
        }
        assert!(store.custom_acp_agents().unwrap().is_empty());
    }

    #[test]
    fn names_paths_and_arguments_are_bounded() {
        let store = SettingsStore::in_memory().unwrap();
        let refused = [
            CustomAcpAgentDraft {
                name: " ".into(),
                ..draft("x")
            },
            CustomAcpAgentDraft {
                name: "n".repeat(65),
                ..draft("x")
            },
            CustomAcpAgentDraft {
                executable: "agent".into(),
                ..draft("x")
            },
            CustomAcpAgentDraft {
                args: vec!["a".into(); 17],
                ..draft("x")
            },
            CustomAcpAgentDraft {
                args: vec!["two\nlines".into()],
                ..draft("x")
            },
            CustomAcpAgentDraft {
                id: Some("custom-acp-5".into()),
                ..draft("x")
            },
        ];
        for draft in &refused {
            assert!(store.save_custom_acp_agent(draft).is_err());
        }
        assert_eq!(custom_acp_slot("custom-acp-4"), Some(4));
        assert_eq!(custom_acp_slot("custom-acp-0"), None);
        assert_eq!(custom_acp_slot("opencode"), None);
    }
}
