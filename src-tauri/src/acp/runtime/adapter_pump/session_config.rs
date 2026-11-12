use super::*;
use crate::acp::goal::GoalAction;
use crate::acp::session_config::ConfigIntent;

pub(crate) fn hydrate_model_config<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) {
    write_discovered_commands(adapter, transport);
    let Ok(Some(egress)) = adapter.model_config_hydration() else {
        return;
    };
    let resolution = EgressResolution::from(transport.write(egress.frame()));
    let _ = adapter.resolve_egress(egress.into_token(), resolution);
}

/// Writes the harness's command-list read when its list is missing or
/// stale. Idle background work, like the model catalog.
pub(crate) fn write_discovered_commands<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) {
    let Ok(Some(egress)) = adapter.discover_commands() else {
        return;
    };
    let resolution = EgressResolution::from(transport.write(egress.frame()));
    let _ = adapter.resolve_egress(egress.into_token(), resolution);
}

/// Writes one deferred fork release while the runtime is idle. It is
/// background work: nothing waits for it, so a failed write only leaves the
/// release wanted for the next idle moment.
pub(crate) fn write_deferred_fork_release<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) {
    let Ok(Some(egress)) = adapter.deferred_fork_release() else {
        return;
    };
    let resolution = EgressResolution::from(transport.write(egress.frame()));
    let _ = adapter.resolve_egress(egress.into_token(), resolution);
}

fn write_egress<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    egress: AdapterEgress,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
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

pub(crate) fn write_usage_read<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) -> Result<(), crate::acp::runtime::HostError> {
    let egress = adapter
        .read_usage()
        .map_err(|_| crate::acp::runtime::HostError::Intent)?;
    write_egress(adapter, transport, egress)
}

pub(crate) fn write_goal<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    action: GoalAction,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let egress = adapter.mutate_goal(action).map_err(|_| HostError::Intent)?;
    write_egress(adapter, transport, egress)
}

/// Writes the native fork request. The fork response can carry the whole
/// forked history, so the decoder may need to tolerate an oversize frame.
pub(crate) fn write_fork<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    // A full fork ledger is a capacity refusal the user can act on.
    let egress = adapter.fork_session().map_err(|error| match error {
        AdapterRuntimeError::BatchCount => HostError::Capacity,
        _ => HostError::Intent,
    })?;
    write_egress(adapter, transport, egress)
}

/// Writes the release of a fork this runtime created. A harness that holds
/// nothing for the fork answers synchronously and writes no frame.
pub(crate) fn write_release_fork<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    fork: &crate::acp::intent::ExternalId,
    committed: bool,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let Some(egress) = adapter
        .release_fork(fork, committed)
        .map_err(|_| HostError::Intent)?
    else {
        return Ok(());
    };
    write_egress(adapter, transport, egress)
}

/// Writes a session control, such as undoing a turn or stopping a task.
pub(crate) fn write_control<A: HarnessAdapter>(
    adapter: &mut A,
    transport: &mut impl EgressTransport,
    control: &crate::acp::session_control::SessionControl,
) -> Result<(), crate::acp::runtime::HostError> {
    use crate::acp::runtime::HostError;
    let egress = adapter
        .session_control(control)
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
    write_egress(adapter, transport, egress)
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
    write_egress(adapter, transport, egress)
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
    /// A Turn is running: the fork release is deferred to the next idle.
    pub(crate) fn defer_fork_release(
        &mut self,
        fork: &crate::acp::intent::ExternalId,
        committed: bool,
    ) -> bool {
        self.adapter.defer_fork_release(fork, committed)
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
        transport: &mut impl EgressTransport,
        action: GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_goal(&mut self.adapter, transport, action)
    }
    pub(crate) fn write_usage_read(
        &mut self,
        transport: &mut impl EgressTransport,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_usage_read(&mut self.adapter, transport)
    }
    pub(crate) fn write_control(
        &mut self,
        transport: &mut impl EgressTransport,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<(), crate::acp::runtime::HostError> {
        self.ensure_live()
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        write_control(&mut self.adapter, transport, control)
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
        if self.idle_decoder.is_frame_aligned() {
            write_deferred_fork_release(&mut self.adapter, transport);
        }
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
        transport: &mut impl EgressTransport,
        action: GoalAction,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_goal(&mut self.adapter, transport, action)
    }
    pub(crate) fn write_usage_read(
        &mut self,
        transport: &mut impl EgressTransport,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_usage_read(&mut self.adapter, transport)
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
    pub(crate) fn write_control(
        &mut self,
        transport: &mut impl EgressTransport,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_control(&mut self.adapter, transport, control)
    }
    pub(crate) fn write_fork(
        &mut self,
        transport: &mut impl EgressTransport,
    ) -> Result<(), crate::acp::runtime::HostError> {
        if !self.idle_decoder.is_frame_aligned() {
            return Err(crate::acp::runtime::HostError::State);
        }
        write_fork(&mut self.adapter, transport)?;
        self.idle_decoder.enable_recoverable_oversize();
        Ok(())
    }
    pub(crate) fn write_release_fork(
        &mut self,
        transport: &mut impl EgressTransport,
        fork: &crate::acp::intent::ExternalId,
        committed: bool,
    ) -> Result<(), crate::acp::runtime::HostError> {
        write_release_fork(&mut self.adapter, transport, fork, committed)
    }
}
