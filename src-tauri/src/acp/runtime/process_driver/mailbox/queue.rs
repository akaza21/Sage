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

    /// The stdout queued behind a completed Turn's durable terminal, in
    /// stream order, when nothing else is queued, no reader terminal is
    /// pending and the mailbox is open. `None` blocks the reusable handoff;
    /// the caller must still prove the returned stdout is safe to leave queued.
    pub(in crate::acp::runtime::process_driver) fn queued_handoff_stdout(
        &self,
    ) -> Option<Vec<Vec<u8>>> {
        let state = self.lock();
        if state.terminal_queued || state.closed {
            return None;
        }
        let mut chunks = Vec::new();
        state
            .queue
            .all_items_match(|message| match message {
                ControlMessage::StdoutChunk(chunk) => {
                    chunks.push(chunk.clone());
                    true
                }
                _ => false,
            })
            .then_some(chunks)
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
