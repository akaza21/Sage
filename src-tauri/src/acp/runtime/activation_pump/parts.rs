use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterRuntimeError, HarnessAdapter, ResourceMapper},
    intent::session_event::PendingSessionEvents,
    limits::LIMITS,
    transport::Frame,
};

use super::ActivatedSession;

pub(crate) struct IdleActivationParts<A, J, R> {
    pub(super) adapter: A,
    pub(super) journal: J,
    pub(super) resources: R,
    pub(super) session: ActivatedSession,
    /// Session events admitted before the first Turn. The first Turn's pump
    /// commits them; see `take_pending_session_events`.
    pending_session_events: PendingSessionEvents,
}

pub(crate) struct RecoveryActivationParts<A, J, R> {
    pub(crate) adapter: A,
    pub(crate) journal: J,
    pub(crate) resources: R,
}

impl<A, J, R> RecoveryActivationParts<A, J, R> {
    pub(crate) fn into_parts(self) -> (A, J, R) {
        (self.adapter, self.journal, self.resources)
    }
}

pub(crate) struct TeardownActivationParts<A, J, R> {
    pub(crate) adapter: A,
    pub(crate) journal: J,
    pub(crate) resources: R,
}

impl<A, J, R> TeardownActivationParts<A, J, R> {
    pub(crate) fn into_parts(self) -> (A, J, R) {
        (self.adapter, self.journal, self.resources)
    }
}

impl<A, J, R> IdleActivationParts<A, J, R> {
    pub(crate) fn new(adapter: A, journal: J, resources: R, session: ActivatedSession) -> Self {
        Self {
            adapter,
            journal,
            resources,
            session,
            pending_session_events: PendingSessionEvents::default(),
        }
    }

    pub(crate) fn with_pending_session_events(mut self, events: PendingSessionEvents) -> Self {
        self.pending_session_events = events;
        self
    }

    /// Hands the buffered session events to the first Turn, which is the
    /// first owner able to commit them durably.
    pub(crate) fn take_pending_session_events(&mut self) -> PendingSessionEvents {
        std::mem::take(&mut self.pending_session_events)
    }

    pub(crate) fn session(&self) -> &ActivatedSession {
        &self.session
    }

    pub(crate) fn into_parts(self) -> (A, J, R, ActivatedSession) {
        (self.adapter, self.journal, self.resources, self.session)
    }

    pub(crate) fn journal(&self) -> &J {
        &self.journal
    }
}

impl<A: HarnessAdapter, J, R: ResourceMapper> IdleActivationParts<A, J, R> {
    pub(crate) fn receive_command_update(
        &mut self,
        frame: &Frame,
    ) -> Result<(), AdapterRuntimeError> {
        self.adapter.receive_command_update(frame)
    }

    pub(crate) fn validate_command_prompt(
        &self,
        prompt: &crate::acp::intent::Prompt,
    ) -> Result<(), crate::acp::runtime::HostError> {
        if let Some(revision) = prompt.command_revision {
            self.adapter
                .available_commands()
                .validate(revision, prompt.as_str())
                .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        }
        Ok(())
    }

    pub(crate) fn hydrate_model_config(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) {
        crate::acp::runtime::adapter_pump::session_config::hydrate_model_config(
            &mut self.adapter,
            transport,
        );
    }

    /// Before the first turn the model catalog is read once, but the
    /// command list is read again whenever the harness reports a change.
    pub(crate) fn write_discovered_commands(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) {
        crate::acp::runtime::adapter_pump::session_config::write_discovered_commands(
            &mut self.adapter,
            transport,
        );
    }

    /// Best-effort read-only hydration after the activation handoff. A Goal
    /// read failure never changes the durable Session or Turn outcome.
    pub(crate) fn hydrate_goal(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) {
        use crate::acp::bindings::adapter::EgressResolution;
        let Ok(Some(egress)) = self.adapter.goal_get() else {
            return;
        };
        let resolution = EgressResolution::from(transport.write(egress.frame()));
        let _ = self.adapter.resolve_egress(egress.into_token(), resolution);
    }

    pub(crate) fn config_pending(&self) -> bool {
        self.adapter.config_pending()
    }
    pub(crate) fn native_rpc_pending(&self) -> bool {
        self.adapter.native_rpc_pending()
    }
    pub(crate) fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.adapter.take_native_rpc_reply()
    }
    pub(crate) fn abandon_usage_read(&mut self) {
        self.adapter.abandon_usage_read();
    }
    pub(crate) fn write_goal(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
        action: crate::acp::goal::GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_goal(
            &mut self.adapter,
            transport,
            action,
        )
    }
    pub(crate) fn write_usage_read(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_usage_read(
            &mut self.adapter,
            transport,
        )
    }
    pub(crate) fn write_control(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_control(
            &mut self.adapter,
            transport,
            control,
        )
    }
    pub(crate) fn write_child(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
        child_thread_id: &str,
        parent_thread_id: &str,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_child(
            &mut self.adapter,
            transport,
            child_thread_id,
            parent_thread_id,
        )
    }
    pub(crate) fn write_release_fork(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
        fork: &crate::acp::intent::ExternalId,
        committed: bool,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_release_fork(
            &mut self.adapter,
            transport,
            fork,
            committed,
        )
    }
    pub(crate) fn write_fork(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_fork(&mut self.adapter, transport)
    }
    pub(crate) fn accept_oversized_response(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        self.adapter.accept_oversized_response(id)
    }
    pub(crate) fn child_read_pending(&self) -> bool {
        self.adapter.child_read_pending()
    }
    pub(crate) fn skips_oversize_frames(&self) -> bool {
        self.adapter.skips_oversize_frames()
    }
    pub(crate) fn write_session_config(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        crate::acp::runtime::adapter_pump::session_config::write_config(
            &mut self.adapter,
            transport,
            intent,
        )
    }
    pub(crate) fn discard_invalid_response(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adapter.discard_invalid_response(frame)
    }
    pub(crate) fn ingest_idle_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adapter.ingest(frame, &self.resources)
    }

    pub(crate) fn accept_idle_batch(
        &mut self,
        batch: AdapterBatch,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) -> Result<(), crate::acp::runtime::AdapterPumpError> {
        use crate::acp::{
            bindings::adapter::{AdapterEvent, EgressResolution},
            runtime::AdapterPumpError,
        };
        let (events, egress) = batch.into_parts();
        let mut catalog = None;
        for event in events {
            match event {
                AdapterEvent::Models {
                    models,
                    next_cursor,
                } if catalog.is_none() => {
                    catalog = Some((models, next_cursor));
                }
                AdapterEvent::Session { id, event } => {
                    let session = &self.session;
                    if id.is_exact_session(
                        &session.binding_digest,
                        session.runtime_epoch,
                        &session.session_id,
                        &session.external_session_id,
                    ) {
                        self.pending_session_events.push(event);
                    }
                }
                AdapterEvent::IgnoredLate
                | AdapterEvent::IgnoredDuplicate
                | AdapterEvent::Absorbed => {}
                _ => return Err(AdapterPumpError::Frame),
            }
        }
        if catalog.is_none()
            && egress.iter().all(|value| {
                value.token().cancelled_permission_request().is_some()
                    || value.token().is_native_rpc_continuation()
            })
        {
            return crate::acp::runtime::adapter_pump::write_idle_egress(
                &mut self.adapter,
                egress,
                transport,
            );
        }
        if egress.len() > 1
            || egress
                .iter()
                .any(|value| value.token().is_control() || value.token().request_id().is_none())
            || (!egress.is_empty() && catalog.as_ref().is_none_or(|(_, cursor)| cursor.is_none()))
        {
            return Err(AdapterPumpError::Frame);
        }
        if let Some((models, next_cursor)) = catalog {
            let count = self
                .session
                .models
                .len()
                .checked_add(models.len())
                .ok_or(AdapterPumpError::BatchCapacity)?;
            if count > LIMITS.wire.models_or_config_entries as usize {
                return Err(AdapterPumpError::BatchCapacity);
            }
            self.session.models.extend(models);
            self.session.next_model_cursor = next_cursor;
        }
        for value in egress {
            let resolution = EgressResolution::from(transport.write(value.frame()));
            self.adapter
                .resolve_egress(value.into_token(), resolution)?;
        }
        Ok(())
    }
}
