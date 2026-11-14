use crate::acp::{runtime::HostError, store::CloseSessionCommand};
use sage_acp_protocol::scheduler::RuntimeOwner;

use super::MailboxError;

pub(super) fn owner_bytes(owner: &RuntimeOwner) -> usize {
    owner.project_id.as_str().len()
        + owner.chat_id.as_str().len()
        + owner.session_id.as_str().len()
        + std::mem::size_of::<u64>()
}

pub(super) fn close_bytes(command: &CloseSessionCommand) -> Option<usize> {
    owner_bytes(&RuntimeOwner {
        project_id: command.project_id.clone(),
        chat_id: command.chat_id.clone(),
        session_id: command.session_id.clone(),
        runtime_epoch: command.runtime_epoch,
    })
    .checked_add(command.client_intent_id.as_str().len())?
    .checked_add(std::mem::size_of::<u64>() + std::mem::size_of::<i64>())
}

pub(super) fn map_mailbox(error: MailboxError) -> HostError {
    match error {
        MailboxError::Full | MailboxError::InvalidChunk => HostError::Capacity,
        MailboxError::Closed => HostError::Closed,
    }
}
