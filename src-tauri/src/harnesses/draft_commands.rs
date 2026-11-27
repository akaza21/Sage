//! What a new-chat draft offers before its session exists: the harness's last
//! reported command list (from a live session, or its passive probe), else its
//! curated built-ins. A command named from it is checked against the session's
//! live list on the first send.

use serde_json::{Value, json};

use super::HarnessCatalog;
use crate::{
    acp::{
        command_placement::KnownCommandTable,
        slash_commands::{AvailableCommand, CommandDispatch},
    },
    harness_discovery::known_harness_spec,
    settings_store::SettingsStore,
};

/// The store's key for a list read outside any project.
const NO_PROJECT: &str = "";

impl HarnessCatalog {
    /// Rows shaped as the live list's commands.
    pub fn draft_commands(
        &self,
        settings: &SettingsStore,
        harness_id: &str,
        project_id: Option<&str>,
    ) -> Vec<Value> {
        let stored = settings
            .draft_commands(harness_id, project_id.unwrap_or(NO_PROJECT))
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str::<Vec<Value>>(&json).ok());
        stored.unwrap_or_else(|| {
            self.draft_builtin_commands(harness_id)
                .into_iter()
                .map(|name| {
                    json!({"name": name, "description": "Built-in", "inputHint": null,
                        "source": "builtin", "placement": {"section": "skillsAndCommands"}})
                })
                .collect()
        })
    }

    /// Stores what the harness's passive probe last read; whether that changed
    /// what drafts offer.
    pub fn record_probed_commands(&self, settings: &SettingsStore, harness_id: &str) -> bool {
        self.registry
            .for_harness(harness_id)
            .and_then(|record| record.probed_commands())
            .is_some_and(|commands| record_draft_commands(settings, harness_id, None, &commands))
    }
}

/// Stores a reported list for this harness's drafts in `project_id`; whether
/// that changed what they offer. An empty list (a harness still starting, or a
/// list that failed to parse) keeps the stored one.
pub(crate) fn record_draft_commands(
    settings: &SettingsStore,
    harness_id: &str,
    project_id: Option<&str>,
    commands: &[AvailableCommand],
) -> bool {
    let table = known_harness_spec(harness_id)
        .and_then(|spec| spec.known_commands)
        .map(|table| table());
    let rows: Vec<Value> = commands
        .iter()
        .filter(|command| runs_from_draft(command, table))
        .filter_map(row)
        .collect();
    if rows.is_empty() {
        return false;
    }
    serde_json::to_string(&rows).is_ok_and(|json| {
        settings
            .record_draft_commands(harness_id, project_id.unwrap_or(NO_PROJECT), &json)
            .unwrap_or(false)
    })
}

/// A command that runs as a first message. One that opens a panel, runs a host
/// action or the binding's own method, or that is a curated native control or
/// terminal-only, needs a started chat, so a draft leaves it out.
fn runs_from_draft(command: &AvailableCommand, table: Option<&KnownCommandTable>) -> bool {
    matches!(
        command.dispatch,
        CommandDispatch::Prompt | CommandDispatch::CommandTurn | CommandDispatch::Skill { .. }
    ) && command.placement.control.is_none()
        && !table.is_some_and(|table| table.needs_session(&command.name))
}

/// The live row without its provenance, which only the Extensions view reads.
fn row(command: &AvailableCommand) -> Option<Value> {
    let mut value = serde_json::to_value(command).ok()?;
    value.as_object_mut()?.remove("sourceInfo");
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::slash_commands::CommandSource;

    fn command(name: &str, source: CommandSource, dispatch: CommandDispatch) -> AvailableCommand {
        AvailableCommand::new(name, "Does a thing", None, source, dispatch).unwrap()
    }

    fn store() -> (std::path::PathBuf, SettingsStore) {
        let folder = std::env::temp_dir().join(format!("sage-drafts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        let store = SettingsStore::open(&folder.join("settings.sqlite")).unwrap();
        (folder, store)
    }

    fn names(rows: &[Value]) -> Vec<&str> {
        rows.iter().filter_map(|row| row["name"].as_str()).collect()
    }

    #[test]
    fn a_draft_offers_the_last_reported_list_without_session_only_commands() {
        let (folder, settings) = store();
        let catalog = HarnessCatalog::default();
        // Nothing reported yet: the curated built-ins.
        assert_eq!(
            names(&catalog.draft_commands(&settings, "codex", Some("p1"))),
            ["skills"]
        );
        let reported = [
            command(
                "review-pr",
                CommandSource::Skill,
                CommandDispatch::Skill { path: "/s".into() },
            ),
            command(
                "deploy:ship",
                CommandSource::Plugin,
                CommandDispatch::Prompt,
            ),
            command("compact", CommandSource::Builtin, CommandDispatch::Action),
            command(
                "status",
                CommandSource::Builtin,
                CommandDispatch::Panel(crate::acp::slash_commands::CommandPanel::Status),
            ),
            command("review", CommandSource::Builtin, CommandDispatch::Prompt),
        ];
        assert!(record_draft_commands(
            &settings,
            "codex",
            Some("p1"),
            &reported
        ));
        assert!(!record_draft_commands(
            &settings,
            "codex",
            Some("p1"),
            &reported
        ));
        let rows = catalog.draft_commands(&settings, "codex", Some("p1"));
        // `review` is a curated native control: it needs a started chat.
        assert_eq!(names(&rows), ["review-pr", "deploy:ship"]);
        assert_eq!(rows[1]["source"], "plugin");
        assert!(rows[0].get("sourceInfo").is_none());
        // Another project, or none, borrows the latest list until its own is reported.
        assert_eq!(
            names(&catalog.draft_commands(&settings, "codex", None)),
            names(&rows)
        );
        // An empty report keeps what was stored.
        assert!(!record_draft_commands(&settings, "codex", Some("p1"), &[]));
        assert_eq!(
            names(&catalog.draft_commands(&settings, "codex", Some("p1"))).len(),
            2
        );
        let _ = std::fs::remove_dir_all(folder);
    }
}
