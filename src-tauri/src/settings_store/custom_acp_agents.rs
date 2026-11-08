//! User-defined ACP agents: a display name, an executable and its
//! arguments, and the names of environment variables to pass through.
//! Only variable names are stored, never their values, so a secret stays
//! in the user's environment. At most four agents exist at once. Each gets
//! a fresh `custom-acp-<n>` id from a persisted counter that never goes
//! back, so chats bound to a removed agent never pass to a later one.

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

/// The number a custom agent id names: `custom-acp-<n>`, `n` in
/// `1..=u32::MAX`, written without sign or leading zeros.
pub fn custom_acp_number(id: &str) -> Option<u32> {
    let digits = id.strip_prefix(ID_PREFIX)?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u32>().ok().filter(|number| *number >= 1)
}

pub fn custom_acp_id(number: u32) -> String {
    format!("{ID_PREFIX}{number}")
}

pub(super) fn migrate(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS custom_acp_agent_entries (
           number INTEGER PRIMARY KEY CHECK (number BETWEEN 1 AND 4294967295),
           name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 256),
           executable TEXT NOT NULL CHECK (length(executable) BETWEEN 1 AND 1024),
           args_json TEXT NOT NULL CHECK (length(args_json) <= 16384),
           env_json TEXT NOT NULL CHECK (length(env_json) <= 4096)
         ) STRICT;
         CREATE TABLE IF NOT EXISTS custom_acp_agent_ids (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           last INTEGER NOT NULL CHECK (last BETWEEN 0 AND 4294967295)
         ) STRICT;
         INSERT OR IGNORE INTO custom_acp_agent_ids(singleton, last) VALUES (1, 0);",
    )?;
    // Pre-release builds kept agents in four reusable slots; carry them over
    // under their ids and start the counter past them.
    let legacy: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='custom_acp_agents')",
        [],
        |row| row.get(0),
    )?;
    if legacy {
        connection.execute_batch(
            "INSERT OR IGNORE INTO custom_acp_agent_entries(number, name, executable, args_json, env_json)
               SELECT slot, name, executable, args_json, env_json FROM custom_acp_agents;
             UPDATE custom_acp_agent_ids
               SET last = max(last, (SELECT coalesce(max(number), 0) FROM custom_acp_agent_entries));
             DROP TABLE custom_acp_agents;",
        )?;
    }
    Ok(())
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
            "SELECT number, name, executable, args_json, env_json
             FROM custom_acp_agent_entries ORDER BY number LIMIT 4",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut agents = Vec::with_capacity(MAX_CUSTOM_ACP_AGENTS);
        for row in rows {
            let (number, name, executable, args, env) = row?;
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
                id: custom_acp_id(number),
                name,
                executable,
                args,
                env,
            });
        }
        Ok(agents)
    }

    /// Updates the agent `draft.id` names, or adds a new agent under the
    /// next id the counter has never issued.
    pub fn save_custom_acp_agent(&self, draft: &CustomAcpAgentDraft) -> AppResult<CustomAcpAgent> {
        let (name, executable, args, env) = validate_custom_acp_agent(draft)?;
        let args_json =
            serde_json::to_string(&args).map_err(|_| invalid("arguments are invalid"))?;
        let env_json =
            serde_json::to_string(&env).map_err(|_| invalid("environment is invalid"))?;
        let mut connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transaction = connection.transaction()?;
        let number = match draft.id.as_deref() {
            Some(id) => {
                let number = custom_acp_number(id).ok_or_else(|| invalid("id is unknown"))?;
                let updated = transaction.execute(
                    "UPDATE custom_acp_agent_entries
                     SET name=?2, executable=?3, args_json=?4, env_json=?5 WHERE number=?1",
                    params![number, name, executable, args_json, env_json],
                )?;
                if updated == 0 {
                    return Err(invalid("id is unknown"));
                }
                number
            }
            None => {
                let number = next_number(&transaction)?;
                transaction.execute(
                    "INSERT INTO custom_acp_agent_entries(number, name, executable, args_json, env_json)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![number, name, executable, args_json, env_json],
                )?;
                number
            }
        };
        transaction.commit()?;
        Ok(CustomAcpAgent {
            id: custom_acp_id(number),
            name,
            executable,
            args,
            env,
        })
    }

    pub fn remove_custom_acp_agent(&self, id: &str) -> AppResult<()> {
        let number = custom_acp_number(id).ok_or_else(|| invalid("id is unknown"))?;
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .execute(
                "DELETE FROM custom_acp_agent_entries WHERE number=?1",
                [number],
            )?;
        Ok(())
    }
}

/// Claims the next id number, refusing a fifth live agent.
fn next_number(connection: &Connection) -> AppResult<u32> {
    let live: i64 =
        connection.query_row("SELECT count(*) FROM custom_acp_agent_entries", [], |row| {
            row.get(0)
        })?;
    if live >= MAX_CUSTOM_ACP_AGENTS as i64 {
        return Err(invalid(
            "limit reached: remove an agent before adding another",
        ));
    }
    let last: u32 = connection
        .query_row(
            "SELECT last FROM custom_acp_agent_ids WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let number = last
        .checked_add(1)
        .ok_or_else(|| invalid("ids are exhausted"))?;
    connection.execute(
        "INSERT INTO custom_acp_agent_ids(singleton, last) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET last=excluded.last",
        [number],
    )?;
    Ok(number)
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
    fn at_most_four_agents_live_and_survive_reopening() {
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
        store.remove_custom_acp_agent("custom-acp-2").unwrap();
        assert_eq!(
            store.save_custom_acp_agent(&draft("Again")).unwrap().id,
            "custom-acp-5",
            "a removed agent's id is never issued again"
        );
        drop(store);
        let store = SettingsStore::open(&path).unwrap();
        let reopened = store.custom_acp_agents().unwrap();
        let ids: Vec<_> = reopened.iter().map(|agent| agent.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "custom-acp-1",
                "custom-acp-3",
                "custom-acp-4",
                "custom-acp-5"
            ]
        );
        assert_eq!(reopened[3].name, "Again");
        assert_eq!(reopened[3].env, ["AGENT_API_KEY"]);
        store.remove_custom_acp_agent("custom-acp-5").unwrap();
        assert_eq!(
            store.save_custom_acp_agent(&draft("Later")).unwrap().id,
            "custom-acp-6",
            "the counter survives reopening and removal of the newest agent"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saving_under_a_removed_or_unissued_id_is_refused() {
        let store = SettingsStore::in_memory().unwrap();
        let saved = store.save_custom_acp_agent(&draft("Agent")).unwrap();
        let renamed = store
            .save_custom_acp_agent(&CustomAcpAgentDraft {
                id: Some(saved.id.clone()),
                ..draft("Renamed")
            })
            .unwrap();
        assert_eq!(renamed.id, saved.id);
        store.remove_custom_acp_agent(&saved.id).unwrap();
        for id in [saved.id.as_str(), "custom-acp-9"] {
            assert!(
                store
                    .save_custom_acp_agent(&CustomAcpAgentDraft {
                        id: Some(id.into()),
                        ..draft("Back")
                    })
                    .is_err()
            );
        }
        assert!(store.custom_acp_agents().unwrap().is_empty());
    }

    #[test]
    fn slot_era_agents_keep_their_ids_and_the_counter_starts_past_them() {
        let directory = crate::test_paths::temp_dir()
            .join(format!("sage-custom-acp-legacy-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE custom_acp_agents (slot INTEGER PRIMARY KEY, name TEXT NOT NULL,
                       executable TEXT NOT NULL, args_json TEXT NOT NULL, env_json TEXT NOT NULL) STRICT;",
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO custom_acp_agents VALUES (3, 'Old', ?1, '[]', '[]')",
                    [draft("x").executable],
                )
                .unwrap();
        }
        let store = SettingsStore::open(&path).unwrap();
        let agents = store.custom_acp_agents().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "custom-acp-3");
        assert_eq!(
            store.save_custom_acp_agent(&draft("New")).unwrap().id,
            "custom-acp-4"
        );
        drop(store);
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
                id: Some("custom-acp-05".into()),
                ..draft("x")
            },
        ];
        for draft in &refused {
            assert!(store.save_custom_acp_agent(draft).is_err());
        }
        assert_eq!(custom_acp_number("custom-acp-4"), Some(4));
        assert_eq!(custom_acp_number("custom-acp-4294967295"), Some(u32::MAX));
        for refused in [
            "custom-acp-0",
            "custom-acp-07",
            "custom-acp-+7",
            "custom-acp-",
            "custom-acp-4294967296",
            "opencode",
        ] {
            assert_eq!(custom_acp_number(refused), None, "{refused}");
        }
    }
}
