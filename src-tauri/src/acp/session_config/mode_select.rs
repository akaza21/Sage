//! Selecting a session mode by id. ACP lets an agent restate its modes as a
//! select option of category `mode`. Claude Code's adapter keeps that option
//! current on every path, and setting it also reports `current_mode_update`,
//! so when the option exists it is both the truth to compare with and the
//! control to set; otherwise the plain session modes are used.

use super::{ConfigId, ConfigKind, ConfigMutation, ConfigOption, ConfigValue, SessionConfig};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ModeSelection {
    /// The session is already in the mode.
    Current,
    /// Apply this mutation; `previous` is the mode it leaves.
    Change {
        previous: Option<String>,
        mutation: ConfigMutation,
    },
    /// The session does not advertise the mode.
    NotOffered,
}

impl SessionConfig {
    fn mode_option(&self) -> Option<&ConfigOption> {
        self.config_options.as_slice().iter().find(|option| {
            option
                .category
                .as_ref()
                .is_some_and(|category| category.as_str() == "mode")
                && matches!(option.kind, ConfigKind::Select { .. })
        })
    }

    /// The session's current mode, preferring the `mode` option.
    pub(crate) fn current_mode(&self) -> Option<&str> {
        match self.mode_option().map(|option| &option.kind) {
            Some(ConfigKind::Select { current_value, .. }) => Some(current_value.as_str()),
            _ => self
                .modes
                .as_ref()
                .map(|modes| modes.current_mode_id.as_str()),
        }
    }

    /// What it takes to put the session in `mode`.
    pub(crate) fn select_mode(&self, mode: &str) -> ModeSelection {
        let current = self.current_mode();
        if current == Some(mode) {
            return ModeSelection::Current;
        }
        let Ok(id) = ConfigId::new("modeId", mode) else {
            return ModeSelection::NotOffered;
        };
        let mutation = match self.mode_option() {
            Some(option) => ConfigMutation::Option {
                config_id: option.id.clone(),
                value: ConfigValue::Select { value: id },
            },
            None => ConfigMutation::Mode(id),
        };
        let offered = match &mutation {
            ConfigMutation::Option { config_id, value } => {
                self.validate_value(config_id.as_str(), value)
            }
            ConfigMutation::Mode(id) => self.validate_mode(id.as_str()),
        };
        if offered.is_err() {
            return ModeSelection::NotOffered;
        }
        ModeSelection::Change {
            previous: current.map(str::to_owned),
            mutation,
        }
    }

    /// Whether `mutation` changes the session mode: a plain mode change, or
    /// a value for the option that restates the modes.
    pub(crate) fn changes_mode(&self, mutation: &ConfigMutation) -> bool {
        match mutation {
            ConfigMutation::Mode(_) => true,
            ConfigMutation::Option { config_id, .. } => {
                self.mode_option()
                    .is_some_and(|option| option.id == *config_id)
                    || Self::may_change_mode(mutation)
            }
        }
    }

    /// The same question without a reported config: any mode change, and an
    /// option named `mode`, are treated as changing the mode.
    pub(crate) fn may_change_mode(mutation: &ConfigMutation) -> bool {
        match mutation {
            ConfigMutation::Mode(_) => true,
            ConfigMutation::Option { config_id, .. } => {
                config_id.as_str().eq_ignore_ascii_case("mode")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(options: serde_json::Value) -> SessionConfig {
        SessionConfig::from_wire(
            &json!({
                "modes": {"currentModeId": "default", "availableModes": [
                    {"id": "default", "name": "Manual"},
                    {"id": "acceptEdits", "name": "Accept edits"},
                    {"id": "plan", "name": "Plan"},
                    {"id": "bypassPermissions", "name": "Bypass permissions"}
                ]},
                "configOptions": options
            }),
            true,
        )
        .unwrap()
    }

    fn mode_option(current: &str) -> serde_json::Value {
        json!([{"id": "mode", "name": "Mode", "category": "mode", "type": "select",
        "currentValue": current, "options": [
            {"value": "default", "name": "Manual"},
            {"value": "acceptEdits", "name": "Accept edits"},
            {"value": "plan", "name": "Plan"},
            {"value": "bypassPermissions", "name": "Bypass permissions"}
        ]}])
    }

    #[test]
    fn the_mode_option_is_set_and_compared_when_the_agent_restates_modes() {
        // Claude's adapter moved the option after a set_mode without a
        // current_mode_update, so the option is the truth.
        let config = config(mode_option("acceptEdits"));
        assert_eq!(config.current_mode(), Some("acceptEdits"));
        assert_eq!(config.select_mode("acceptEdits"), ModeSelection::Current);
        assert_eq!(
            config.select_mode("bypassPermissions"),
            ModeSelection::Change {
                previous: Some("acceptEdits".into()),
                mutation: ConfigMutation::Option {
                    config_id: ConfigId::new("id", "mode").unwrap(),
                    value: ConfigValue::Select {
                        value: ConfigId::new("id", "bypassPermissions").unwrap()
                    },
                },
            }
        );
    }

    #[test]
    fn plain_session_modes_are_used_without_a_mode_option() {
        let config = config(json!([]));
        assert_eq!(config.select_mode("default"), ModeSelection::Current);
        assert_eq!(
            config.select_mode("plan"),
            ModeSelection::Change {
                previous: Some("default".into()),
                mutation: ConfigMutation::Mode(ConfigId::new("id", "plan").unwrap()),
            }
        );
    }

    #[test]
    fn a_mode_the_session_does_not_advertise_is_never_approximated() {
        for config in [config(json!([])), config(mode_option("default"))] {
            assert_eq!(config.select_mode("auto"), ModeSelection::NotOffered);
            assert_eq!(config.select_mode(""), ModeSelection::NotOffered);
        }
        assert_eq!(
            SessionConfig::default().select_mode("default"),
            ModeSelection::NotOffered
        );
    }

    #[test]
    fn mode_changes_are_recognised_by_kind_and_by_the_mode_option() {
        let id = |value: &str| ConfigId::new("id", value).unwrap();
        let option = |config_id: &str| ConfigMutation::Option {
            config_id: id(config_id),
            value: ConfigValue::Select {
                value: id("bypassPermissions"),
            },
        };
        let restated = config(mode_option("default"));
        assert!(restated.changes_mode(&ConfigMutation::Mode(id("bypassPermissions"))));
        assert!(restated.changes_mode(&option("mode")));
        assert!(!restated.changes_mode(&option("model")));
        assert!(SessionConfig::may_change_mode(&option("Mode")));
        assert!(!SessionConfig::may_change_mode(&option("effort")));
    }
}
