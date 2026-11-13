use super::*;
use crate::acp::session_config::ConfigIntent;

impl<A, R, T, C> FirstTurnWorkerState<A, R, T, C>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    pub(crate) fn begin_configure<Auth>(
        &mut self,
        context: &FirstTurnWorkerContext<Auth>,
        intent: &ConfigIntent,
        deadline: std::time::Instant,
    ) -> Result<(), HostError> {
        if context.mailbox.reader_terminal_queued() {
            return Err(HostError::RecoveryRequired);
        }
        if std::time::Instant::now() >= deadline {
            return Err(HostError::Deadline);
        }
        if intent.owner != context.owner {
            return Err(HostError::Intent);
        }
        if self.config_pending() {
            return Err(HostError::State);
        }
        match self {
            Self::Idle(idle) => idle.parts.write_session_config(&mut idle.transport, intent),
            Self::Reusable(parts) => parts
                .completed
                .write_session_config(&mut parts.transport, intent),
            Self::Active(active) if active.stdout_allowed() => {
                active.turn.driver.begin_configure_session(intent)
            }
            _ => Err(HostError::State),
        }
    }

    pub(crate) fn config_pending(&self) -> bool {
        match self {
            Self::Idle(idle) => idle.parts.config_pending(),
            Self::Reusable(parts) => parts.completed.config_pending(),
            Self::Active(active) => active.turn.driver.config_pending(),
            _ => false,
        }
    }

    pub(crate) fn native_rpc_pending(&self) -> bool {
        match self {
            Self::Idle(idle) => idle.parts.native_rpc_pending(),
            Self::Reusable(parts) => parts.completed.native_rpc_pending(),
            Self::Active(active) => active.turn.driver.native_rpc_pending(),
            _ => false,
        }
    }

    pub(crate) fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        match self {
            Self::Idle(idle) => idle.parts.take_native_rpc_reply(),
            Self::Reusable(parts) => parts.completed.take_native_rpc_reply(),
            Self::Active(active) => active.turn.driver.take_native_rpc_reply(),
            _ => None,
        }
    }

    pub(crate) fn abandon_usage_read(&mut self) {
        match self {
            Self::Idle(idle) => idle.parts.abandon_usage_read(),
            Self::Reusable(parts) => parts.completed.abandon_usage_read(),
            Self::Active(active) => active.turn.driver.abandon_usage_read(),
            _ => {}
        }
    }

    pub(crate) fn begin_native_rpc<Auth>(
        &mut self,
        context: &FirstTurnWorkerContext<Auth>,
        intent: &crate::acp::goal::NativeRpcIntent,
        deadline: std::time::Instant,
    ) -> Result<(), HostError> {
        if context.mailbox.reader_terminal_queued() || std::time::Instant::now() >= deadline {
            return Err(HostError::RecoveryRequired);
        }
        if intent.owner != context.owner || self.native_rpc_pending() {
            return Err(HostError::Intent);
        }
        match self {
            Self::Idle(idle) => match &intent.request {
                crate::acp::goal::NativeRpcRequest::Goal(action) => {
                    idle.parts.write_goal(&mut idle.transport, *action)
                }
                crate::acp::goal::NativeRpcRequest::Read(read) => {
                    if !idle.idle_decoder.is_frame_aligned() {
                        return Err(HostError::State);
                    }
                    idle.parts.write_child(&mut idle.transport, read)?;
                    idle.idle_decoder.enable_recoverable_oversize();
                    Ok(())
                }
                crate::acp::goal::NativeRpcRequest::ForkSession => {
                    if !idle.idle_decoder.is_frame_aligned() {
                        return Err(HostError::State);
                    }
                    idle.parts.write_fork(&mut idle.transport)?;
                    idle.idle_decoder.enable_recoverable_oversize();
                    Ok(())
                }
                crate::acp::goal::NativeRpcRequest::ReleaseFork { fork, committed } => idle
                    .parts
                    .write_release_fork(&mut idle.transport, fork, *committed),
                crate::acp::goal::NativeRpcRequest::UsageRead => {
                    idle.parts.write_usage_read(&mut idle.transport)
                }
                crate::acp::goal::NativeRpcRequest::Control(control) => {
                    idle.parts.write_control(&mut idle.transport, control)
                }
            },
            Self::Reusable(parts) => match &intent.request {
                crate::acp::goal::NativeRpcRequest::Goal(action) => {
                    parts.completed.write_goal(&mut parts.transport, *action)
                }
                crate::acp::goal::NativeRpcRequest::Read(read) => {
                    parts.completed.write_child(&mut parts.transport, read)
                }
                crate::acp::goal::NativeRpcRequest::ForkSession => {
                    parts.completed.write_fork(&mut parts.transport)
                }
                crate::acp::goal::NativeRpcRequest::ReleaseFork { fork, committed } => parts
                    .completed
                    .write_release_fork(&mut parts.transport, fork, *committed),
                crate::acp::goal::NativeRpcRequest::UsageRead => {
                    parts.completed.write_usage_read(&mut parts.transport)
                }
                crate::acp::goal::NativeRpcRequest::Control(control) => {
                    parts.completed.write_control(&mut parts.transport, control)
                }
            },
            Self::Active(active) if active.stdout_allowed() => {
                active.turn.driver.begin_native_rpc(intent)
            }
            _ => Err(HostError::State),
        }
    }
}
