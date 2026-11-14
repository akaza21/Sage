use sage_acp_protocol::{
    scheduler::RuntimeOwner,
    session_config::{
        ConfigIntent, ConfigMutation, ConfigRevision, SessionConfig, SessionConfigChangedV1,
        SessionConfigOverlay,
    },
};
use std::sync::{Arc, Mutex};

mod restore_gate;
#[cfg(test)]
mod tests;

struct Entry {
    owner: RuntimeOwner,
    live: bool,
    overlay: Option<SessionConfigOverlay>,
    tombstone: Option<SessionConfigOverlay>,
    restore_pending: bool,
    new_session: bool,
    /// The restore has started, so what the harness reports is a choice.
    restoring: bool,
    hydration: restore_gate::Hydration,
    /// Stored choices the restore has not reached yet. What the harness
    /// reports for them is not recorded, so a value it resets on its own
    /// (an effort a model change resets) does not replace the stored one.
    held: Vec<ConfigMutation>,
}
#[derive(Default)]
struct State {
    entry: Option<Entry>,
    pending: Option<SessionConfigChangedV1>,
}
pub(crate) type NoticeSink = Arc<dyn Fn(SessionConfigChangedV1) + Send + Sync>;
pub(crate) type ChoiceRecorder = Arc<dyn Fn(&RuntimeOwner, &[ConfigMutation]) + Send + Sync>;
pub(crate) type SnapshotRecorder = Arc<dyn Fn(&RuntimeOwner, &SessionConfig) + Send + Sync>;
#[derive(Default)]
pub(crate) struct ConfigChannel {
    state: Arc<Mutex<State>>,
    sink: Mutex<Option<NoticeSink>>,
    recorder: Mutex<Option<ChoiceRecorder>>,
    snapshot: Mutex<Option<SnapshotRecorder>>,
    notice_scheduled: Arc<std::sync::atomic::AtomicBool>,
    /// Woken whenever what a waiting restore reads may have changed.
    changed: std::sync::Condvar,
}
impl ConfigChannel {
    pub(crate) fn set_sink(&self, sink: NoticeSink) {
        *self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
    }
    pub(crate) fn set_recorder(&self, recorder: ChoiceRecorder) {
        *self
            .recorder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(recorder);
    }
    pub(crate) fn set_snapshot_recorder(&self, recorder: SnapshotRecorder) {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(recorder);
    }
    fn keep_snapshot(&self, owner: &RuntimeOwner, config: Option<SessionConfig>) {
        let recorder = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some((recorder, config)) = recorder.zip(config) {
            recorder(owner, &config);
        }
    }
    pub(crate) fn reserve(&self, owner: RuntimeOwner) {
        let mut state = self.lock();
        state.entry = Some(Entry {
            owner,
            live: false,
            overlay: None,
            tombstone: None,
            restore_pending: true,
            new_session: false,
            restoring: false,
            hydration: restore_gate::Hydration::Done,
            held: Vec::new(),
        });
        state.pending = None;
        self.changed.notify_all();
    }
    pub(crate) fn publish(&self, owner: &RuntimeOwner) {
        let mut state = self.lock();
        let mut snapshot = None;
        if let Some(entry) = &mut state.entry
            && &entry.owner == owner
        {
            entry.live = true;
            snapshot = entry.overlay.as_ref().map(|overlay| overlay.config.clone());
        }
        Self::queue_notice(&mut state);
        drop(state);
        self.flush_notice();
        self.keep_snapshot(owner, snapshot);
    }
    pub(crate) fn update(&self, owner: &RuntimeOwner, config: SessionConfig) -> Result<(), String> {
        config.validate()?;
        let mut state = self.lock();
        let Some(entry) = &mut state.entry else {
            return Ok(());
        };
        if &entry.owner != owner || entry.tombstone.is_some() {
            return Ok(());
        }
        if entry
            .overlay
            .as_ref()
            .is_some_and(|old| old.config == config)
        {
            return Ok(());
        }
        let revision = entry
            .overlay
            .as_ref()
            .map_or(Ok(ConfigRevision::first()), |old| {
                old.config_revision.next()
            })?;
        // Once restored, what the harness reports itself (a mode it switched
        // to, an effort a model change reset) is the session's choice too,
        // except for a stored choice the restore has yet to reapply.
        let mut reported = entry
            .overlay
            .as_ref()
            .filter(|_| entry.live && entry.restoring)
            .map(|old| sage_acp_protocol::session_config::reported::changes(&old.config, &config))
            .unwrap_or_default();
        let snapshot = entry.live.then(|| config.clone());
        reported.retain(|value| {
            !entry
                .held
                .iter()
                .any(|held| sage_acp_protocol::session_config::reported::same_target(held, value))
        });
        entry.overlay = Some(SessionConfigOverlay {
            sage_agent_session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            config_revision: revision,
            removed: false,
            config,
        });
        entry.tombstone = None;
        Self::queue_notice(&mut state);
        drop(state);
        self.flush_notice();
        self.record(owner, &reported);
        self.keep_snapshot(owner, snapshot);
        Ok(())
    }
    fn record(&self, owner: &RuntimeOwner, choices: &[ConfigMutation]) {
        if choices.is_empty() {
            return;
        }
        let recorder = self
            .recorder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(recorder) = recorder {
            recorder(owner, choices);
        }
    }
    pub(crate) fn mark_new_session(&self, owner: &RuntimeOwner) {
        if let Some(entry) = self.lock().entry.as_mut().filter(|e| &e.owner == owner) {
            entry.new_session = true;
        }
    }
    /// The restore has reached `choice`: what the harness reports for it is
    /// recorded again.
    pub(crate) fn release(&self, owner: &RuntimeOwner, choice: &ConfigMutation) {
        if let Some(entry) = self.lock().entry.as_mut().filter(|e| &e.owner == owner) {
            entry.held.retain(|held| {
                !sage_acp_protocol::session_config::reported::same_target(held, choice)
            });
        }
    }
    /// Makes a partly restored runtime finish its restore before its next
    /// prompt. The choices it has not reached stay held, and so do `hold`
    /// (choices stored since it began), so a report does not replace them.
    pub(crate) fn rearm_restore(&self, owner: &RuntimeOwner, hold: &[ConfigMutation]) {
        if let Some(entry) = self.lock().entry.as_mut().filter(|e| &e.owner == owner) {
            entry.restore_pending = true;
            for choice in hold {
                let same = |held: &ConfigMutation| {
                    sage_acp_protocol::session_config::reported::same_target(held, choice)
                };
                entry.held.retain(|held| !same(held));
                entry.held.push(choice.clone());
            }
        }
    }
    pub(crate) fn snapshot(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay> {
        self.lock()
            .entry
            .as_ref()
            .filter(|e| e.live && &e.owner == owner)
            .and_then(|e| e.overlay.clone().or_else(|| e.tombstone.clone()))
    }
    pub(crate) fn live_snapshot(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay> {
        self.lock()
            .entry
            .as_ref()
            .filter(|e| e.live && &e.owner == owner)
            .and_then(|e| e.overlay.clone())
    }
    pub(crate) fn remove(&self, owner: &RuntimeOwner) -> Result<(), String> {
        let mut state = self.lock();
        let Some(entry) = &mut state.entry else {
            return Ok(());
        };
        if &entry.owner != owner || entry.overlay.is_none() {
            return Ok(());
        }
        let revision = entry
            .overlay
            .as_ref()
            .expect("checked overlay")
            .config_revision
            .next()?;
        entry.overlay = None;
        entry.tombstone = Some(SessionConfigOverlay {
            sage_agent_session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            config_revision: revision,
            removed: true,
            config: SessionConfig::default(),
        });
        Self::queue_notice(&mut state);
        drop(state);
        self.flush_notice();
        Ok(())
    }
    pub(crate) fn validate(&self, intent: &ConfigIntent) -> Result<(), String> {
        let overlay = self
            .live_snapshot(&intent.owner)
            .ok_or("no live session config")?;
        if overlay.config_revision != intent.expected_config_revision {
            return Err("stale session config revision".into());
        }
        match &intent.mutation {
            ConfigMutation::Mode(mode) => overlay.config.validate_mode(mode.as_str()),
            ConfigMutation::Option { config_id, value } => {
                overlay.config.validate_value(config_id.as_str(), value)
            }
        }
    }
    pub(crate) fn clear(&self, owner: &RuntimeOwner) {
        let mut state = self.lock();
        if state.entry.as_ref().is_some_and(|e| &e.owner == owner) {
            state.entry = None;
            state.pending = None;
        }
        self.changed.notify_all();
    }
    pub(crate) fn clear_all(&self) {
        *self.lock() = State::default();
        self.changed.notify_all();
    }
    #[cfg(test)]
    pub(crate) fn take_notice(&self) -> Option<SessionConfigChangedV1> {
        self.lock().pending.take()
    }
    fn queue_notice(state: &mut State) {
        if let Some(entry) = &state.entry
            && entry.live
            && let Some(overlay) = entry.overlay.as_ref().or(entry.tombstone.as_ref())
        {
            state.pending = Some(SessionConfigChangedV1 {
                schema_version: 1,
                sage_project_id: entry.owner.project_id.clone(),
                sage_chat_id: entry.owner.chat_id.clone(),
                sage_agent_session_id: entry.owner.session_id.clone(),
                runtime_epoch: entry.owner.runtime_epoch,
                config_revision: overlay.config_revision,
            });
        }
    }
    fn flush_notice(&self) {
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(sink) = sink else {
            return;
        };
        if self
            .notice_scheduled
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let state = Arc::clone(&self.state);
        let scheduled = Arc::clone(&self.notice_scheduled);
        sage_core::rt::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                let notice = {
                    let mut state = state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match state.pending.take() {
                        Some(notice) => notice,
                        None => {
                            scheduled.store(false, std::sync::atomic::Ordering::Release);
                            return;
                        }
                    }
                };
                sink(notice);
            }
        });
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
