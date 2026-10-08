use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterRuntimeError, HarnessAdapter, ResourceMapper},
    transport::Frame,
};

use super::ActivatedSession;

pub(crate) struct IdleActivationParts<A, J, R> {
    pub(super) adapter: A,
    pub(super) journal: J,
    pub(super) resources: R,
    pub(super) session: ActivatedSession,
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
        }
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
    /// Best-effort read-only hydration after the activation handoff. A Goal
    /// read failure never changes the durable Session or Turn outcome.
    pub(crate) fn hydrate_goal(
        &mut self,
        transport: &mut impl crate::acp::runtime::EgressTransport,
    ) {
        use crate::acp::{bindings::adapter::EgressResolution, transport::FrameWriteOutcome};
        let Ok(Some(egress)) = self.adapter.goal_get() else {
            return;
        };
        let resolution = match transport.write(egress.frame()) {
            FrameWriteOutcome::Complete => EgressResolution::Written,
            FrameWriteOutcome::ZeroByteFailure => EgressResolution::WriteZeroBytes,
            FrameWriteOutcome::DeliveryUnknown { .. } => EgressResolution::DeliveryUnknown,
        };
        let _ = self.adapter.resolve_egress(egress.into_token(), resolution);
    }

    pub(crate) fn config_pending(&self) -> bool {
        self.adapter.config_pending()
    }
    pub(crate) fn goal_pending(&self) -> bool {
        self.adapter.goal_pending()
    }
    pub(crate) fn native_rpc_pending(&self) -> bool {
        self.adapter.native_rpc_pending()
    }
    pub(crate) fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.adapter.take_native_rpc_reply()
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
    pub(crate) fn accept_oversized_response(&mut self, id: u64) -> Result<(), AdapterRuntimeError> {
        self.adapter.accept_oversized_response(id)
    }
    pub(crate) fn child_read_pending(&self) -> bool {
        self.adapter.child_read_pending()
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
    pub(crate) fn ingest_idle_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.adapter.ingest(frame, &self.resources)
    }
}
