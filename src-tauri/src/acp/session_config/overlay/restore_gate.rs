//! What a restore of the stored choices reads from the channel, and when it
//! may start.
use std::time::Instant;

use super::{ConfigChannel, ConfigMutation, RuntimeOwner, SessionConfig, SessionConfigOverlay};

impl ConfigChannel {
    /// Whether the session of the runtime `owner` names is still reading
    /// what it offers (a catalogue its options come from). Its stored
    /// choices are not restored until it has.
    pub(crate) fn set_hydrating(&self, owner: &RuntimeOwner, hydrating: bool) {
        let mut state = self.lock();
        if let Some(entry) = state.entry.as_mut().filter(|e| &e.owner == owner)
            && entry.hydrating != hydrating
        {
            entry.hydrating = hydrating;
            drop(state);
            self.changed.notify_all();
        }
    }
    /// Hands out, exactly once, the live overlay of a runtime whose stored
    /// choices have not been reapplied, with whether it started a new
    /// session. It stays pending while the session is hydrating and until
    /// `ready` accepts what it reports, so a harness that reports its
    /// options after activation is restored before its next prompt instead.
    /// `held` are the stored choices it is about to reapply; see `release`.
    pub(crate) fn take_restore(
        &self,
        owner: &RuntimeOwner,
        held: &[ConfigMutation],
        ready: impl FnOnce(&SessionConfig) -> bool,
    ) -> Option<(SessionConfigOverlay, bool)> {
        let mut state = self.lock();
        let entry = state
            .entry
            .as_mut()
            .filter(|e| e.live && e.restore_pending && !e.hydrating && &e.owner == owner)?;
        let overlay = entry.overlay.as_ref().filter(|o| ready(&o.config))?.clone();
        entry.restore_pending = false;
        entry.restoring = true;
        entry.held = held.to_vec();
        Some((overlay, entry.new_session))
    }
    /// Waits, until `deadline`, while the runtime `owner` names has stored
    /// choices to restore and its session is still reading the options they
    /// need, so a prompt sent right after activation does not go out ahead
    /// of them. Returns at once otherwise.
    pub(crate) fn wait_while_hydrating(&self, owner: &RuntimeOwner, deadline: Instant) {
        let mut state = self.lock();
        loop {
            let hydrating = state
                .entry
                .as_ref()
                .is_some_and(|e| e.live && e.restore_pending && e.hydrating && &e.owner == owner);
            let Some(left) = deadline
                .checked_duration_since(Instant::now())
                .filter(|_| hydrating)
            else {
                return;
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
        session_config::{ConfigId, ConfigValue},
    };
    use std::{sync::Arc, time::Duration};

    fn owner() -> RuntimeOwner {
        RuntimeOwner {
            project_id: SageProjectId::parse("project-gate").unwrap(),
            chat_id: SageChatId::parse("chat-gate").unwrap(),
            session_id: SageAgentSessionId::parse("session-gate").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        }
    }

    fn effort() -> SessionConfig {
        SessionConfig::from_wire(
            &serde_json::json!({"configOptions":[{
                "id":"effort","name":"Effort","type":"select","currentValue":"low",
                "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]
            }]}),
            true,
        )
        .unwrap()
    }

    fn stored() -> ConfigMutation {
        ConfigMutation::Option {
            config_id: ConfigId::new("configId", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "high").unwrap(),
            },
        }
    }

    #[test]
    fn a_restore_waits_while_the_session_reads_what_it_offers() {
        let channel = Arc::new(ConfigChannel::default());
        let runtime = owner();
        channel.reserve(runtime.clone());
        channel.update(&runtime, effort()).unwrap();
        channel.set_hydrating(&runtime, true);
        channel.publish(&runtime);
        // Options are reported, but more are on their way.
        assert!(
            channel
                .take_restore(&runtime, &[stored()], |_| true)
                .is_none()
        );
        let waiter = std::thread::spawn({
            let channel = Arc::clone(&channel);
            let runtime = runtime.clone();
            move || {
                let started = Instant::now();
                channel.wait_while_hydrating(&runtime, started + Duration::from_secs(30));
                started.elapsed()
            }
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(!waiter.is_finished(), "the restore waits for the options");
        channel.set_hydrating(&runtime, false);
        assert!(waiter.join().unwrap() < Duration::from_secs(30));
        assert!(
            channel
                .take_restore(&runtime, &[stored()], |_| true)
                .is_some()
        );
    }

    #[test]
    fn a_wait_ends_at_its_deadline_or_at_once_with_nothing_to_restore() {
        let channel = ConfigChannel::default();
        let runtime = owner();
        channel.reserve(runtime.clone());
        channel.update(&runtime, effort()).unwrap();
        channel.set_hydrating(&runtime, true);
        // Not live yet: nothing waits on it.
        let started = Instant::now();
        channel.wait_while_hydrating(&runtime, started + Duration::from_secs(30));
        assert!(started.elapsed() < Duration::from_secs(30));
        channel.publish(&runtime);
        let deadline = Instant::now() + Duration::from_millis(20);
        channel.wait_while_hydrating(&runtime, deadline);
        assert!(Instant::now() >= deadline);
    }
}
