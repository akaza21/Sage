use super::*;
use crate::acp::{
    background_terminals::{BackgroundTerminal, TerminalAction},
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::{HostError, RuntimeHost},
    scheduler::RuntimeOwner,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalsError {
    /// The harness refused the request or was busy; nothing changed.
    Rejected,
    /// The request may have reached the harness and its outcome is unknown.
    RecoveryRequired,
}

impl AcpService {
    /// Runs `action` against the runtime's harness, then answers with the
    /// terminals it still runs. A runtime that is gone, or that never
    /// negotiated background terminals, runs none.
    pub(crate) fn background_terminals(
        &self,
        owner: RuntimeOwner,
        action: TerminalAction,
    ) -> Result<Vec<BackgroundTerminal>, TerminalsError> {
        let Some(host) = self
            .lane_for(&owner)
            .and_then(|lane| lane.runtime.for_owner(&owner).ok())
        else {
            return Ok(Vec::new());
        };
        if action != TerminalAction::List && self.terminals_rpc(&owner, &host, action)?.is_none() {
            return Ok(Vec::new());
        }
        Ok(self
            .terminals_rpc(&owner, &host, TerminalAction::List)?
            .unwrap_or_default())
    }

    /// `None` when the runtime cannot serve terminals at all.
    fn terminals_rpc(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        action: TerminalAction,
    ) -> Result<Option<Vec<BackgroundTerminal>>, TerminalsError> {
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::BackgroundTerminals(action),
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::Deadline
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner.clone(), Arc::clone(host));
        }
        terminals_reply(result)
    }
}

fn terminals_reply(
    result: Result<NativeRpcReply, HostError>,
) -> Result<Option<Vec<BackgroundTerminal>>, TerminalsError> {
    match result {
        Ok(NativeRpcReply::BackgroundTerminals(terminals)) => Ok(Some(terminals)),
        Ok(NativeRpcReply::TerminalsStopped) => Ok(Some(Vec::new())),
        Ok(NativeRpcReply::Rejected) | Err(HostError::State | HostError::NotDispatched) => {
            Err(TerminalsError::Rejected)
        }
        Err(HostError::Intent) => Ok(None),
        Ok(_) | Err(_) => Err(TerminalsError::RecoveryRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_terminal_replies_answer_and_an_unsupported_runtime_runs_none() {
        let terminal = BackgroundTerminal {
            handle: "7".into(),
            command: "npm run dev".into(),
            cwd: None,
        };
        assert_eq!(
            terminals_reply(Ok(NativeRpcReply::BackgroundTerminals(vec![
                terminal.clone()
            ]))),
            Ok(Some(vec![terminal]))
        );
        assert_eq!(
            terminals_reply(Ok(NativeRpcReply::TerminalsStopped)),
            Ok(Some(Vec::new()))
        );
        assert_eq!(terminals_reply(Err(HostError::Intent)), Ok(None));
        for refused in [
            Ok(NativeRpcReply::Rejected),
            Err(HostError::State),
            Err(HostError::NotDispatched),
        ] {
            assert_eq!(terminals_reply(refused), Err(TerminalsError::Rejected));
        }
        for unknown in [
            Ok(NativeRpcReply::UsageRead),
            Err(HostError::DeliveryUnknown),
            Err(HostError::Closed),
        ] {
            assert_eq!(
                terminals_reply(unknown),
                Err(TerminalsError::RecoveryRequired)
            );
        }
    }
}
