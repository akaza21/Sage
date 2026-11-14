//! How the Agent sidebar is laid out: organized by project or in one list,
//! sorted by last update or manual order. One closed record, so the WebView
//! keeps only the rendered projection.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::AppResult;
use serde::{Deserialize, Serialize};

use super::SettingsStore;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentRailOrganize {
    #[default]
    Project,
    List,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentRailOrdering {
    #[default]
    Recency,
    Manual,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentRailView {
    pub organize: AgentRailOrganize,
    pub ordering: AgentRailOrdering,
}

pub(super) fn migrate(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS agent_rail_view (
           singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
           organize TEXT NOT NULL CHECK (organize IN ('project', 'list')),
           ordering TEXT NOT NULL CHECK (ordering IN ('recency', 'manual'))
         ) STRICT;",
    )?;
    Ok(())
}

impl SettingsStore {
    pub fn agent_rail_view(&self) -> AppResult<AgentRailView> {
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stored = connection
            .query_row(
                "SELECT organize, ordering FROM agent_rail_view WHERE singleton = 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        Ok(stored
            .map(|(organize, ordering)| AgentRailView {
                organize: if organize == "list" {
                    AgentRailOrganize::List
                } else {
                    AgentRailOrganize::Project
                },
                ordering: if ordering == "manual" {
                    AgentRailOrdering::Manual
                } else {
                    AgentRailOrdering::Recency
                },
            })
            .unwrap_or_default())
    }

    pub fn set_agent_rail_view(&self, value: AgentRailView) -> AppResult<AgentRailView> {
        let organize = match value.organize {
            AgentRailOrganize::Project => "project",
            AgentRailOrganize::List => "list",
        };
        let ordering = match value.ordering {
            AgentRailOrdering::Recency => "recency",
            AgentRailOrdering::Manual => "manual",
        };
        let connection = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        connection.execute(
            "INSERT INTO agent_rail_view(singleton, organize, ordering) VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               organize = excluded.organize,
               ordering = excluded.ordering",
            params![organize, ordering],
        )?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_rail_view_defaults_to_projects_by_last_update() {
        let store = SettingsStore::in_memory().unwrap();
        assert_eq!(
            store.agent_rail_view().unwrap(),
            AgentRailView {
                organize: AgentRailOrganize::Project,
                ordering: AgentRailOrdering::Recency,
            }
        );
    }

    #[test]
    fn the_agent_rail_view_survives_reopening_the_store() {
        let directory =
            std::env::temp_dir().join(format!("sage-settings-rail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.db");
        let value = AgentRailView {
            organize: AgentRailOrganize::List,
            ordering: AgentRailOrdering::Manual,
        };
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .set_agent_rail_view(value)
                .unwrap(),
            value
        );
        assert_eq!(
            SettingsStore::open(&path)
                .unwrap()
                .agent_rail_view()
                .unwrap(),
            value
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn the_agent_rail_view_refuses_unknown_fields_and_values() {
        assert!(
            serde_json::from_str::<AgentRailView>(
                r#"{"organize":"list","ordering":"manual","path":"x"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<AgentRailView>(r#"{"organize":"status","ordering":"manual"}"#)
                .is_err()
        );
    }
}
