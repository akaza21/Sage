use super::*;

const PASSIVE_RPC_DEADLINE_MS: u64 = 5_000;

impl HostCommand {
    pub(in crate::acp::runtime::runtime_host) fn deadline_millis(&self) -> u64 {
        use crate::acp::limits::LIMITS;
        match self {
            Self::NativeRpc(intent) if intent.passive() => PASSIVE_RPC_DEADLINE_MS,
            Self::ConfigureSession(_) | Self::NativeRpc(_) | Self::ResolvePermission { .. } => {
                LIMITS.deadlines_ms.ordinary_rpc
            }
            Self::Activate(_) => LIMITS.deadlines_ms.initialize,
            Self::Cancel(_) => LIMITS.deadlines_ms.tree_termination,
            Self::Terminal { .. }
            | Self::BeginClose(_)
            | Self::FinishClose(_)
            | Self::RecoverClose(_)
            | Self::AcceptDurableCompleted
            | Self::BeginPark
            | Self::FinishPark
            | Self::Shutdown => LIMITS.deadlines_ms.explicit_close,
            Self::PreparePrompt(_)
            | Self::DispatchPrompt { .. }
            | Self::Steer(_)
            | Self::ProjectSwitched(_)
            | Self::State => LIMITS.deadlines_ms.ordinary_rpc,
        }
    }

    /// A command that holds a user prompt, with whatever images it carries.
    pub(in crate::acp::runtime::runtime_host) fn carries_prompt(&self) -> bool {
        matches!(self, Self::PreparePrompt(_) | Self::Steer(_))
    }

    pub(in crate::acp::runtime::runtime_host) fn is_control(&self) -> bool {
        matches!(
            self,
            Self::ResolvePermission { .. }
                | Self::Cancel(_)
                | Self::Terminal { .. }
                | Self::State
                | Self::BeginClose(_)
                | Self::FinishClose(_)
                | Self::RecoverClose(_)
                | Self::AcceptDurableCompleted
                | Self::BeginPark
                | Self::FinishPark
                | Self::Shutdown
        )
    }

    pub(in crate::acp::runtime::runtime_host) fn actual_bytes(&self) -> usize {
        fn owner_bytes(owner: &RuntimeOwner) -> usize {
            owner.project_id.as_str().len()
                + owner.chat_id.as_str().len()
                + owner.session_id.as_str().len()
                + std::mem::size_of::<u64>()
        }
        match self {
            Self::ConfigureSession(intent) => intent.actual_bytes(),
            Self::NativeRpc(intent) => intent.actual_bytes(),
            Self::ResolvePermission {
                owner, request_id, ..
            } => owner_bytes(owner) + request_id.as_str().len() + 1,
            Self::Activate(owner) => owner_bytes(owner),
            Self::PreparePrompt(intent) => {
                owner_bytes(&intent.owner())
                    + intent.command.client_intent_id.as_str().len()
                    + intent.command.prompt.retained_bytes()
                    + std::mem::size_of::<u64>() * 2
            }
            Self::DispatchPrompt { owner, turn_id } => owner_bytes(owner) + turn_id.as_str().len(),
            Self::Steer(intent) => {
                owner_bytes(&intent.owner)
                    + intent.turn_id.as_str().len()
                    + intent.prompt.retained_bytes()
                    + intent.client_message_id.as_str().len()
            }
            Self::Cancel(intent) => {
                owner_bytes(&intent.owner)
                    + intent.turn_id.as_str().len()
                    + intent.client_intent_id.as_str().len()
                    + std::mem::size_of::<i64>()
            }
            Self::BeginClose(intent) | Self::FinishClose(intent) | Self::RecoverClose(intent) => {
                owner_bytes(&intent.owner())
                    + intent.command.client_intent_id.as_str().len()
                    + std::mem::size_of::<u64>()
                    + std::mem::size_of::<i64>()
            }
            Self::Terminal { owner, cursor } => {
                owner_bytes(owner) + cursor.turn_id().as_str().len() + 32
            }
            Self::ProjectSwitched(project) => project.as_str().len(),
            Self::State
            | Self::AcceptDurableCompleted
            | Self::BeginPark
            | Self::FinishPark
            | Self::Shutdown => 1,
        }
    }
}
