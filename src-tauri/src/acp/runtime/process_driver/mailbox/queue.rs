use crate::acp::transport::{QueueError, ReservedQueue};

use super::{ControlMessage, MailboxError};

pub(super) fn pop_driver_message(
    queue: &mut ReservedQueue<ControlMessage>,
    control_only: bool,
) -> Option<ControlMessage> {
    if let Some(message) =
        queue.pop_control_if(|message| !matches!(message, ControlMessage::ProcessExit(_)))
    {
        return Some(message);
    }
    if control_only {
        return None;
    }
    if let Some(message) =
        queue.pop_data_if(|message| matches!(message, ControlMessage::StdoutChunk(_)))
    {
        return Some(message);
    }
    if let Some(message) =
        queue.pop_control_if(|message| matches!(message, ControlMessage::ProcessExit(_)))
    {
        return Some(message);
    }
    queue.pop()
}

pub(super) fn map_queue(error: QueueError) -> MailboxError {
    match error {
        QueueError::CountCapacity
        | QueueError::ByteCapacity
        | QueueError::ControlItemTooLarge
        | QueueError::SequenceCapacity => MailboxError::Full,
    }
}

pub(super) fn reader_chunk_limit() -> usize {
    8 * 1024
}
