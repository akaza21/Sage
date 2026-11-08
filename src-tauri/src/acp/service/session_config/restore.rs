//! Keeps a chat's session config choices across park, restart and fork.
//!
//! Every binding exposes its config through the same seam: the runtime
//! publishes what the session reports into the lane's `ConfigChannel`, and a
//! change goes through `AcpService::configure_session` to the binding's
//! `HarnessAdapter::configure`. So the choices are stored here, and
//! reapplied here through that same path, with no harness branch.
use std::time::{Duration, Instant};

use crate::acp::{
    limits::LIMITS,
    runtime::HostError,
    scheduler::RuntimeOwner,
    session_config::{
        ChoiceRecorder, ConfigIntent, ConfigMutation, SessionConfig, SessionConfigOverlay, choices,
        reported::current_value,
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

/// Mode first, then options that pick the model, then the rest, so a model
/// is restored before an effort that depends on it. The option's category,
/// not its ID, says what it is.
fn priority(config: &SessionConfig, choice: &ConfigMutation) -> u8 {
    let ConfigMutation::Option { config_id, .. } = choice else {
        return 0;
    };
    let category = config
        .config_options
        .as_slice()
        .iter()
        .find(|option| &option.id == config_id)
        .and_then(|option| option.category.as_ref());
    match category.map(|category| category.as_str()) {
        Some("mode") => 0,
        Some("model") => 1,
        _ => 2,
    }
}

/// Whether the session reports what the stored choices need: its modes when
/// a mode is stored, its options when an option is stored.
fn ready(config: &SessionConfig, stored: &[ConfigMutation]) -> bool {
    let options = !config.config_options.as_slice().is_empty();
    let modes = config.modes.is_some();
    (modes || options)
        && stored.iter().all(|choice| match choice {
            ConfigMutation::Mode(_) => modes,
            ConfigMutation::Option { .. } => options,
        })
}

fn needs_recovery(error: &HostError) -> bool {
    matches!(
        error,
        HostError::Unavailable
            | HostError::Closed
            | HostError::Deadline
            | HostError::DeliveryUnknown
            | HostError::RecoveryRequired
    )
}

impl AcpService {
    /// Stores the values sessions report themselves, once restored.
    pub(in crate::acp::service) fn session_config_recorder(&self) -> ChoiceRecorder {
        let database = self.database.clone();
        std::sync::Arc::new(move |owner: &RuntimeOwner, reported: &[ConfigMutation]| {
            for choice in reported {
                remember(&database, owner, choice);
            }
        })
    }

    pub(super) fn remember_config_choice(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        remember(&self.database, owner, choice);
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

    /// Reapplies the session's stored choices to a new runtime, once the
    /// session reports the modes and options they need. Each stored value
    /// that differs and is still offered is sent through the same config path
    /// the composer uses. A value the session no longer offers, or one the
    /// harness refuses, falls back to the session's value, and the stored
    /// value is updated to match. A new session stores the mode it starts in.
    ///
    /// The whole restore gets one ordinary RPC deadline. What is left when it
    /// runs out is restored before the next prompt, and until then what the
    /// harness reports for those choices does not replace them. `Err` means
    /// a change left the runtime needing recovery.
    pub(crate) fn restore_session_config(&self, owner: &RuntimeOwner) -> Result<(), HostError> {
        let deadline = Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.ordinary_rpc);
        self.restore_until(owner, deadline, || Instant::now() >= deadline)
    }

    /// The restore, sending changes until `out_of_time`, each answered by
    /// `deadline`.
    pub(super) fn restore_until(
        &self,
        owner: &RuntimeOwner,
        deadline: Instant,
        mut out_of_time: impl FnMut() -> bool,
    ) -> Result<(), HostError> {
        let Some(lane) = self.lane_for(owner) else {
            return Ok(());
        };
        let config = &lane.runtime.config;
        let mut stored = match choices::load(&self.database.connection(), &owner.session_id) {
            Ok(stored) => stored,
            Err(error) => {
                tracing::warn!(?error, "stored session config choices were not read");
                return Ok(());
            }
        };
        let Some((mut overlay, new_session)) =
            config.take_restore(owner, &stored, |reported| ready(reported, &stored))
        else {
            return Ok(());
        };
        // Whichever way the restore stops short, the rest of it runs before
        // the next prompt, and its choices stay held until then.
        let mut unfinished = Unfinished {
            config,
            owner,
            finished: false,
        };
        // A new session stores the mode it starts in, unless it already has
        // one (a fork keeps its parent's).
        if new_session
            && !stored
                .iter()
                .any(|choice| matches!(choice, ConfigMutation::Mode(_)))
            && let Some(modes) = &overlay.config.modes
        {
            self.remember_config_choice(
                owner,
                &ConfigMutation::Mode(modes.current_mode_id.clone()),
            );
        }
        stored.sort_by_key(|choice| priority(&overlay.config, choice));
        for choice in stored {
            match step(&overlay.config, &choice) {
                Step::Keep => {}
                Step::Forget | Step::Fallback(_) => self.fall_back(owner, &overlay.config, &choice),
                Step::Apply => {
                    if out_of_time() {
                        tracing::warn!(
                            "session config restore ran out of time; finishing before the next prompt"
                        );
                        return Ok(());
                    }
                    match self.apply_stored(owner, &overlay, &choice, deadline)? {
                        Applied::Sent(latest) => overlay = latest,
                        Applied::Deferred | Applied::Gone => return Ok(()),
                    }
                }
            }
            config.release(owner, &choice);
        }
        unfinished.finished = true;
        Ok(())
    }

    /// Sends one stored value, retrying once if another change raced it.
    fn apply_stored(
        &self,
        owner: &RuntimeOwner,
        overlay: &SessionConfigOverlay,
        choice: &ConfigMutation,
        deadline: Instant,
    ) -> Result<Applied, HostError> {
        let Some(lane) = self.lane_for(owner) else {
            return Ok(Applied::Gone);
        };
        let apply = |revision| {
            self.configure_session_until(
                ConfigIntent {
                    owner: owner.clone(),
                    expected_config_revision: revision,
                    mutation: choice.clone(),
                },
                Some(deadline),
            )
        };
        let mut result = apply(overlay.config_revision);
        if result == Err(HostError::Intent) {
            // A change raced this one: retry once against the new state.
            let Some(latest) = lane.runtime.config.live_snapshot(owner) else {
                return Ok(Applied::Gone);
            };
            result = match step(&latest.config, choice) {
                Step::Apply => apply(latest.config_revision),
                Step::Keep => Ok(()),
                Step::Forget | Step::Fallback(_) => Err(HostError::ConfigMutationRejected),
            };
        }
        match &result {
            Ok(()) | Err(HostError::ConfigMutationRejected) => {}
            Err(error) if needs_recovery(error) => return Err(error.clone()),
            Err(error) => {
                // Busy, full, raced again, or out of time before it was sent
                // (`NotDispatched`): nothing changed, so it is tried again
                // before the next prompt. Only a change that was sent and
                // went unanswered needs recovery.
                tracing::warn!(?error, "session config restore deferred");
                return Ok(Applied::Deferred);
            }
        }
        let Some(latest) = lane.runtime.config.live_snapshot(owner) else {
            return Ok(Applied::Gone);
        };
        if result == Err(HostError::ConfigMutationRejected) {
            self.fall_back(owner, &latest.config, choice);
        }
        Ok(Applied::Sent(latest))
    }
}

enum Applied {
    /// The harness answered; this is the session's config after it.
    Sent(SessionConfigOverlay),
    /// Not sent now; tried again before the next prompt.
    Deferred,
    /// The runtime is gone.
    Gone,
}

/// Rearms a restore that stops before its last choice.
struct Unfinished<'a> {
    config: &'a crate::acp::session_config::ConfigChannel,
    owner: &'a RuntimeOwner,
    finished: bool,
}

impl Drop for Unfinished<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.config.rearm_restore(self.owner);
        }
    }
}

fn remember(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
    choice: &ConfigMutation,
) {
    let stored = database.write_connection().and_then(|connection| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::session_config::{ConfigId, ConfigValue};
    use serde_json::json;

    fn config() -> SessionConfig {
        SessionConfig::from_wire(
            &json!({
                "modes": {"currentModeId": "default", "availableModes": [
                    {"id": "default", "name": "Default"}, {"id": "plan", "name": "Plan"}
                ]},
                "configOptions": [{
                    "id": "effort", "name": "Effort", "category": "thought_level",
                    "type": "select", "currentValue": "low",
                    "options": [{"value": "low", "name": "Low"}, {"value": "high", "name": "High"}]
                }, {
                    "id": "pick", "name": "Model", "category": "model", "type": "select",
                    "currentValue": "a", "options": [{"value": "a", "name": "A"}]
                }, {"id": "fast", "name": "Fast", "type": "boolean", "currentValue": false}]
            }),
            true,
        )
        .unwrap()
    }

    fn option(id: &str, value: ConfigValue) -> ConfigMutation {
        ConfigMutation::Option {
            config_id: ConfigId::new("configId", id).unwrap(),
            value,
        }
    }

    fn select(value: &str) -> ConfigValue {
        ConfigValue::Select {
            value: ConfigId::new("value", value).unwrap(),
        }
    }

    fn mode(id: &str) -> ConfigMutation {
        ConfigMutation::Mode(ConfigId::new("mode", id).unwrap())
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

    #[test]
    fn the_mode_and_the_model_are_restored_before_what_depends_on_them() {
        let config = config();
        let mut stored = vec![
            option("effort", select("high")),
            option("fast", ConfigValue::Boolean { value: true }),
            option("pick", select("a")),
            mode("plan"),
        ];
        stored.sort_by_key(|choice| priority(&config, choice));
        assert_eq!(
            stored,
            [
                mode("plan"),
                option("pick", select("a")),
                option("effort", select("high")),
                option("fast", ConfigValue::Boolean { value: true }),
            ]
        );
    }

    #[test]
    fn stored_options_wait_for_the_session_to_report_options() {
        let modes_only = SessionConfig {
            config_options: Default::default(),
            ..config()
        };
        assert!(ready(&modes_only, &[mode("plan")]));
        assert!(!ready(
            &modes_only,
            &[mode("plan"), option("effort", select("high"))]
        ));
        assert!(ready(
            &config(),
            &[mode("plan"), option("effort", select("high"))]
        ));
        assert!(!ready(&SessionConfig::default(), &[]));
        assert!(ready(&modes_only, &[]));
    }
}
