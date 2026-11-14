use super::*;
use crate::acp::{
    background_terminals::{BackgroundTerminal, TerminalAction},
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::HostError,
    scheduler::RuntimeOwner,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalsError {
    /// The runtime is no longer the current one; nothing was sent.
    Stale,
    /// The harness refused the request or was busy; nothing changed.
    Rejected,
    /// The request may have reached the harness and its outcome is unknown.
    RecoveryRequired,
}

impl AcpService {
    /// Runs `action` against the runtime's harness. A listing answers with
    /// the terminals it still runs, or none when the runtime is gone. A stop
    /// answers with none, and the caller lists again; a stop for a runtime
    /// that is gone is `Stale`, since nothing stopped. A harness that is busy,
    /// holds back a listing or never negotiated terminals refuses either.
    pub(crate) fn background_terminals(
        &self,
        owner: RuntimeOwner,
        action: TerminalAction,
    ) -> Result<Vec<BackgroundTerminal>, TerminalsError> {
        let list = action == TerminalAction::List;
        let Some(host) = self
            .lane_for(&owner)
            .and_then(|lane| lane.runtime.for_owner(&owner).ok())
        else {
            return absent_runtime(&action);
        };
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::BackgroundTerminals(action),
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        terminals_reply(list, result)
    }
}

/// A runtime that is gone runs no terminals, and a stop aimed at it reached
/// nothing, so it must not read as done.
pub(crate) fn absent_runtime(
    action: &TerminalAction,
) -> Result<Vec<BackgroundTerminal>, TerminalsError> {
    match action {
        TerminalAction::List => Ok(Vec::new()),
        TerminalAction::Stop(_) | TerminalAction::StopAll => Err(TerminalsError::Stale),
    }
}

/// A listing that misses its deadline was abandoned, so nothing changed; a
/// stop that misses it may still land.
fn terminals_reply(
    list: bool,
    result: Result<NativeRpcReply, HostError>,
) -> Result<Vec<BackgroundTerminal>, TerminalsError> {
    match result {
        Ok(NativeRpcReply::BackgroundTerminals(terminals)) => Ok(terminals),
        Ok(NativeRpcReply::TerminalsStopped) => Ok(Vec::new()),
        Ok(NativeRpcReply::Rejected) | Err(HostError::State | HostError::Intent) => {
            Err(TerminalsError::Rejected)
        }
        Err(HostError::Deadline) if list => Err(TerminalsError::Rejected),
        Ok(_) | Err(_) => Err(TerminalsError::RecoveryRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_terminal_replies_answer_and_a_refused_request_never_reads_as_done() {
        let terminal = BackgroundTerminal {
            handle: "7".into(),
            command: "npm run dev".into(),
            cwd: None,
        };
        assert_eq!(
            terminals_reply(
                true,
                Ok(NativeRpcReply::BackgroundTerminals(vec![terminal.clone()]))
            ),
            Ok(vec![terminal])
        );
        assert_eq!(
            terminals_reply(false, Ok(NativeRpcReply::TerminalsStopped)),
            Ok(Vec::new())
        );
        for (list, refused) in [
            (true, Err(HostError::Intent)),
            (true, Ok(NativeRpcReply::Rejected)),
            (false, Err(HostError::State)),
            (false, Err(HostError::Intent)),
            (true, Err(HostError::Deadline)),
        ] {
            assert_eq!(
                terminals_reply(list, refused),
                Err(TerminalsError::Rejected)
            );
        }
        for unknown in [
            Ok(NativeRpcReply::UsageRead),
            Err(HostError::DeliveryUnknown),
            Err(HostError::Closed),
            Err(HostError::Deadline),
        ] {
            assert_eq!(
                terminals_reply(false, unknown),
                Err(TerminalsError::RecoveryRequired)
            );
        }
    }
}
