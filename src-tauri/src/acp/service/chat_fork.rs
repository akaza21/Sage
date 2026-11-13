use crate::acp::{
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    intent::ExternalId,
    runtime::HostError,
    scheduler::RuntimeOwner,
    store::chat_fork::ForkSource,
};

use super::AcpService;

/// Why a native fork produced no new session. Every variant leaves the
/// parent Chat untouched and creates no Sage Chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ChatForkError {
    #[error("This chat can't be forked until its harness is running and idle.")]
    Unavailable,
    #[error("The harness declined to fork this chat.")]
    Rejected,
    #[error("This chat is too long to fork.")]
    TooLarge,
    #[error("The harness did not confirm the fork; try again.")]
    RecoveryRequired,
    #[error(
        "This chat's harness still holds too many earlier forks. Let its current turn finish, then try again."
    )]
    TooManyForks,
}

impl AcpService {
    /// Asks the source session's own live runtime for a native fork through
    /// its latest Turn and returns the new native session id. Nothing durable
    /// changes here; the caller installs the fork Chat and its lineage.
    pub(crate) fn fork_native_session(
        &self,
        source: &ForkSource,
    ) -> Result<ExternalId, ChatForkError> {
        let owner = RuntimeOwner {
            project_id: source.project_id.clone(),
            chat_id: source.chat_id.clone(),
            session_id: source.session_id.clone(),
            runtime_epoch: source.runtime_epoch,
        };
        let host = self
            .host_for(&owner)
            .map_err(|_| ChatForkError::Unavailable)?;
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::ForkSession,
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::Deadline
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        fork_reply(result)
    }

    /// Best-effort release of a native fork from the source runtime once its
    /// fork Chat is committed or refused. Nothing durable depends on it: a
    /// release that cannot run leaves the fork loaded in the source runtime,
    /// bounded per runtime by the binding, until that runtime exits.
    pub(crate) fn release_native_fork(
        &self,
        source: &ForkSource,
        fork: &ExternalId,
        committed: bool,
    ) -> bool {
        let owner = RuntimeOwner {
            project_id: source.project_id.clone(),
            chat_id: source.chat_id.clone(),
            session_id: source.session_id.clone(),
            runtime_epoch: source.runtime_epoch,
        };
        let Ok(host) = self.host_for(&owner) else {
            return false;
        };
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::ReleaseFork {
                fork: fork.clone(),
                committed,
            },
        });
        if matches!(
            result,
            Err(HostError::DeliveryUnknown | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        matches!(result, Ok(NativeRpcReply::Released))
    }
}

fn fork_reply(result: Result<NativeRpcReply, HostError>) -> Result<ExternalId, ChatForkError> {
    match result {
        Ok(NativeRpcReply::Forked(external)) => Ok(external),
        Ok(NativeRpcReply::TooLarge) => Err(ChatForkError::TooLarge),
        Ok(NativeRpcReply::Rejected) => Err(ChatForkError::Rejected),
        Ok(
            NativeRpcReply::GoalApplied
            | NativeRpcReply::ChildTranscript(_)
            | NativeRpcReply::Released
            | NativeRpcReply::UsageRead
            | NativeRpcReply::Controlled(_)
            | NativeRpcReply::ControlRefused(_)
            | NativeRpcReply::McpServers(_)
            | NativeRpcReply::McpSignIn(_),
        ) => Err(ChatForkError::RecoveryRequired),
        Err(HostError::Intent | HostError::State) => Err(ChatForkError::Unavailable),
        Err(HostError::Capacity) => Err(ChatForkError::TooManyForks),
        Err(_) => Err(ChatForkError::RecoveryRequired),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_forked_reply_yields_a_native_session() {
        let external = ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap();
        assert_eq!(
            fork_reply(Ok(NativeRpcReply::Forked(external.clone()))),
            Ok(external)
        );
        assert_eq!(
            fork_reply(Ok(NativeRpcReply::TooLarge)),
            Err(ChatForkError::TooLarge)
        );
        assert_eq!(
            fork_reply(Ok(NativeRpcReply::Rejected)),
            Err(ChatForkError::Rejected)
        );
        assert_eq!(
            fork_reply(Ok(NativeRpcReply::GoalApplied)),
            Err(ChatForkError::RecoveryRequired)
        );
        // A runtime that refuses the RPC outright (busy, unsupported harness)
        // is unavailable, not a recovery case.
        assert_eq!(
            fork_reply(Err(HostError::State)),
            Err(ChatForkError::Unavailable)
        );
        assert_eq!(
            fork_reply(Err(HostError::Deadline)),
            Err(ChatForkError::RecoveryRequired)
        );
        // A full per-runtime fork ledger says what the user can do about it.
        assert_eq!(
            fork_reply(Err(HostError::Capacity)),
            Err(ChatForkError::TooManyForks)
        );
        assert!(
            ChatForkError::TooManyForks
                .to_string()
                .contains("too many earlier forks")
        );
    }
}
