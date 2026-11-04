use crate::acp::transport::{QueueError, ReservedQueue};

use super::{ControlMessage, MailboxError};

#[derive(Clone, Copy)]
pub(super) enum ReceiveMode {
    All,
    Control,
    WriteObservation,
}

impl super::DriverMailbox {
    pub(in crate::acp::runtime::process_driver) fn diagnostic_state(&self) -> String {
        let state = self.lock();
        format!(
            "closed={} reader_terminal={} data={:?} control={:?}",
            state.closed,
            state.terminal_queued,
            state.queue.data_usage(),
            state.queue.control_usage(),
        )
    }
}

pub(super) fn pop_driver_message(
    queue: &mut ReservedQueue<ControlMessage>,
    mode: ReceiveMode,
) -> Option<ControlMessage> {
    if let Some(message) =
        queue.pop_control_if(|message| !matches!(message, ControlMessage::ProcessExit(_)))
    {
        return Some(message);
    }
    if matches!(mode, ReceiveMode::Control) {
        return None;
    }
    // Native output can beat the host's write-observation round trip. Retain
    // it (and any following EOF/exit) until that reserved control arrives.
    if matches!(mode, ReceiveMode::WriteObservation)
        && matches!(queue.data_front(), Some(ControlMessage::StdoutChunk(_)))
    {
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
