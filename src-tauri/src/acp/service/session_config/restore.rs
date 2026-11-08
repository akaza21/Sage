//! Keeps a chat's session config choices across park and restart.
//!
//! Every binding exposes its config through the same seam: the runtime
//! publishes what the session reports into the lane's `ConfigChannel`, and a
//! change goes through `AcpService::configure_session` to the binding's
//! `HarnessAdapter::configure`. So the choice the harness confirmed is stored
//! here, and reapplied here through that same path, with no harness branch.
use crate::acp::{
    runtime::HostError,
    scheduler::RuntimeOwner,
    session_config::{
        ConfigIntent, ConfigKind, ConfigMutation, ConfigValue, SessionConfig, choices,
    },
};

use super::AcpService;

/// What one stored choice needs against the options the session reports now.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// The session already runs with the stored value.
    Keep,
    /// The value is still offered and differs: send it.
    Apply,
    /// The option is offered but the value is not: store the session's value.
    Fallback(ConfigMutation),
    /// The session no longer offers the option at all.
    Forget,
}

/// The session's current value for the mode or option `choice` names, or
/// `None` when the session does not offer it.
fn current_value(config: &SessionConfig, choice: &ConfigMutation) -> Option<ConfigMutation> {
    match choice {
        ConfigMutation::Mode(_) => config
            .modes
            .as_ref()
            .map(|modes| ConfigMutation::Mode(modes.current_mode_id.clone())),
        ConfigMutation::Option { config_id, .. } => {
            let option = config
                .config_options
                .as_slice()
                .iter()
                .find(|option| &option.id == config_id)?;
            let value = match &option.kind {
                ConfigKind::Select { current_value, .. } => ConfigValue::Select {
                    value: current_value.clone(),
                },
                ConfigKind::Boolean { current_value } => ConfigValue::Boolean {
                    value: *current_value,
                },
                ConfigKind::Unsupported { .. } => return None,
            };
            Some(ConfigMutation::Option {
                config_id: config_id.clone(),
                value,
            })
        }
    }
}

fn step(config: &SessionConfig, stored: &ConfigMutation) -> Step {
    let Some(current) = current_value(config, stored) else {
        return Step::Forget;
    };
    let offered = match stored {
        ConfigMutation::Mode(mode) => config.validate_mode(mode.as_str()),
        ConfigMutation::Option { config_id, value } => {
            config.validate_value(config_id.as_str(), value)
        }
    }
    .is_ok();
    if !offered {
        Step::Fallback(current)
    } else if &current == stored {
        Step::Keep
    } else {
        Step::Apply
    }
}

impl AcpService {
    pub(super) fn remember_config_choice(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        let stored = self.database.write_connection().and_then(|connection| {
            choices::record(
                &connection,
                &owner.session_id,
                choice,
                crate::persistence::now_millis(),
            )
            .map_err(Into::into)
        });
        if let Err(error) = stored {
            tracing::warn!(?error, "session config choice was not stored");
        }
    }

    fn forget_config_choice(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        let removed = self.database.write_connection().and_then(|connection| {
            choices::forget(&connection, &owner.session_id, choice).map_err(Into::into)
        });
        if let Err(error) = removed {
            tracing::warn!(?error, "stale session config choice was not removed");
        }
    }

    /// Stores the session's own value for `choice`, or drops the stored one
    /// when the session no longer offers that mode or option.
    fn fall_back(&self, owner: &RuntimeOwner, config: &SessionConfig, choice: &ConfigMutation) {
        match current_value(config, choice) {
            Some(current) => self.remember_config_choice(owner, &current),
            None => self.forget_config_choice(owner, choice),
        }
    }

    /// Reapplies the session's stored choices to a new runtime, once, after
    /// the session first reports its modes or options. Each stored value that
    /// differs and is still offered is sent through the same config path the
    /// composer uses. A value the session no longer offers, or one the harness
    /// refuses, falls back to the session's value, and the stored value is
    /// updated to match; nothing is retried.
    pub(crate) fn restore_session_config(&self, owner: &RuntimeOwner) {
        let Some(lane) = self.lane_for(owner) else {
            return;
        };
        let Some(mut overlay) = lane.runtime.config.take_restore(owner) else {
            return;
        };
        let stored = match choices::load(&self.database.connection(), &owner.session_id) {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(?error, "stored session config choices were not read");
                return;
            }
        };
        for choice in stored {
            match step(&overlay.config, &choice) {
                Step::Keep => {}
                Step::Forget | Step::Fallback(_) => self.fall_back(owner, &overlay.config, &choice),
                Step::Apply => {
                    let result = self.configure_session(ConfigIntent {
                        owner: owner.clone(),
                        expected_config_revision: overlay.config_revision,
                        mutation: choice.clone(),
                    });
                    let Some(latest) = lane.runtime.config.live_snapshot(owner) else {
                        return;
                    };
                    overlay = latest;
                    match result {
                        Ok(()) => {}
                        Err(HostError::ConfigMutationRejected) => {
                            self.fall_back(owner, &overlay.config, &choice)
                        }
                        // The runtime needs recovery, which owns what happens
                        // next; the stored values stay for the next resume.
                        Err(
                            HostError::Unavailable
                            | HostError::Closed
                            | HostError::Deadline
                            | HostError::DeliveryUnknown
                            | HostError::RecoveryRequired,
                        ) => return,
                        // A concurrent change won the revision race; the
                        // stored value stays for the next resume.
                        Err(_) => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> SessionConfig {
        SessionConfig::from_wire(
            &json!({
                "modes": {"currentModeId": "default", "availableModes": [
                    {"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}
                ]},
                "configOptions": [{
                    "id": "effort", "name": "Effort", "type": "select", "currentValue": "low",
                    "options": [{"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]
                }, {"id": "fast", "name": "Fast", "type": "boolean", "currentValue": false}]
            }),
            true,
        )
        .unwrap()
    }

    fn option(id: &str, value: ConfigValue) -> ConfigMutation {
        ConfigMutation::Option {
            config_id: crate::acp::session_config::ConfigId::new("configId", id).unwrap(),
            value,
        }
    }

    fn select(value: &str) -> ConfigValue {
        ConfigValue::Select {
            value: crate::acp::session_config::ConfigId::new("value", value).unwrap(),
        }
    }

    fn mode(id: &str) -> ConfigMutation {
        ConfigMutation::Mode(crate::acp::session_config::ConfigId::new("mode", id).unwrap())
    }

    #[test]
    fn each_stored_choice_is_kept_applied_replaced_or_forgotten() {
        let config = config();
        assert_eq!(step(&config, &option("effort", select("low"))), Step::Keep);
        assert_eq!(
            step(&config, &option("effort", select("high"))),
            Step::Apply
        );
        assert_eq!(
            step(&config, &option("effort", select("max"))),
            Step::Fallback(option("effort", select("low")))
        );
        assert_eq!(
            step(
                &config,
                &option("fast", ConfigValue::Boolean { value: true })
            ),
            Step::Apply
        );
        // A select value stored for what is now a boolean is not offered.
        assert_eq!(
            step(&config, &option("fast", select("on"))),
            Step::Fallback(option("fast", ConfigValue::Boolean { value: false }))
        );
        assert_eq!(step(&config, &option("model", select("a"))), Step::Forget);
        assert_eq!(step(&config, &mode("plan")), Step::Apply);
        assert_eq!(step(&config, &mode("default")), Step::Keep);
        assert_eq!(
            step(&config, &mode("gone")),
            Step::Fallback(mode("default"))
        );
        assert_eq!(step(&SessionConfig::default(), &mode("plan")), Step::Forget);
    }
}
