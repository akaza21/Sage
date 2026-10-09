use super::*;
use crate::acp::goal::GoalAction;
use crate::acp::session_config::ConfigIntent;

pub(crate) fn hydrate_model_config<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) {
    let Ok(Some(egress)) = adapter.model_config_hydration() else {
        return;
    };
    let resolution = EgressResolution::from(transport.write(egress.frame()));
    let _ = adapter.resolve_egress(egress.into_token(), resolution);
}

pub(crate) fn write_goal<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    action: GoalAction,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let egress = adapter.mutate_goal(action).map_err(|_| HostError::Intent)?;
    let outcome = transport.write(egress.frame());
    let resolution = EgressResolution::from(outcome);
    adapter
        .resolve_egress(egress.into_token(), resolution)
        .map_err(|_| HostError::RecoveryRequired)?;
    match outcome {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure => Err(HostError::Unavailable),
        FrameWriteOutcome::DeliveryUnknown { .. } => Err(HostError::DeliveryUnknown),
    }
}

pub(crate) fn write_child<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    child_thread_id: &str,
    parent_thread_id: &str,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let egress = adapter
        .read_child(child_thread_id, parent_thread_id)
        .map_err(|_| HostError::Intent)?;
    let outcome = transport.write(egress.frame());
    let resolution = EgressResolution::from(outcome);
    adapter
        .resolve_egress(egress.into_token(), resolution)
        .map_err(|_| HostError::RecoveryRequired)?;
    match outcome {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure => Err(HostError::Unavailable),
        FrameWriteOutcome::DeliveryUnknown { .. } => Err(HostError::DeliveryUnknown),
    }
}

pub(crate) fn write_config<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    intent: &ConfigIntent,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let Some(egress) = adapter.configure(intent).map_err(|_| HostError::Intent)? else {
        return Ok(());
    };
    let outcome = transport.write(egress.frame());
    let resolution = EgressResolution::from(outcome);
    adapter
        .resolve_egress(egress.into_token(), resolution)
        .map_err(|_| HostError::RecoveryRequired)?;
    match outcome {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure => Err(HostError::Unavailable),
        FrameWriteOutcome::DeliveryUnknown { .. } => Err(HostError::DeliveryUnknown),
    }
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn write_session_config(
        &mut self,
        transport: &mut impl EgressTransport,
        intent: &ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_config(&mut self.adapter, transport, intent)
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
        transport: &mut impl EgressTransport,
        action: GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_goal(&mut self.adapter, transport, action)
    }
    pub(crate) fn write_child(
        &mut self,
        transport: &mut impl EgressTransport,
        child_thread_id: &str,
        parent_thread_id: &str,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        if !self.decoder.is_frame_aligned() {
            return Err(crate::acp::runtime::HostError::State);
        }
        write_child(
            &mut self.adapter,
            transport,
            child_thread_id,
            parent_thread_id,
        )?;
        self.decoder.enable_recoverable_oversize();
        Ok(())
    }
}
impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(crate) fn hydrate_model_config(&mut self, transport: &mut impl EgressTransport) {
        hydrate_model_config(&mut self.adapter, transport);
    }

    pub(crate) fn write_session_config(
        &mut self,
        transport: &mut impl EgressTransport,
        intent: &ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_config(&mut self.adapter, transport, intent)
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
        transport: &mut impl EgressTransport,
        action: GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_goal(&mut self.adapter, transport, action)
    }
    pub(crate) fn write_child(
        &mut self,
        transport: &mut impl EgressTransport,
        child_thread_id: &str,
        parent_thread_id: &str,
    ) -> Result<(), crate::acp::runtime::HostError> {
        if !self.idle_decoder.is_frame_aligned() {
            return Err(crate::acp::runtime::HostError::State);
        }
        write_child(
            &mut self.adapter,
            transport,
            child_thread_id,
            parent_thread_id,
        )?;
        self.idle_decoder.enable_recoverable_oversize();
        Ok(())
    }
}
