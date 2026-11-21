use super::ControlMessage;
use crate::acp::runtime::process_driver::MailboxError;

pub(super) fn control_kind(message: &ControlMessage) -> &'static str {
    match message {
        ControlMessage::StdoutChunk(_) => "stdout",
        ControlMessage::Prompt(_) => "prompt",
        ControlMessage::Cancel(_) => "cancel",
        ControlMessage::Worker(_) => "worker",
        ControlMessage::ReaderTerminal(crate::acp::runtime::ReaderTerminal::Eof) => "reader-eof",
        ControlMessage::ReaderTerminal(crate::acp::runtime::ReaderTerminal::Io) => "reader-io",
        ControlMessage::ProcessExit(_) => "process-exit",
    }
}

pub(super) fn log_unexpected_mailbox(
    checkpoint: &'static str,
    result: &Result<Option<ControlMessage>, MailboxError>,
) {
    tracing::warn!(
        activation_checkpoint = checkpoint,
        outcome = ?result
            .as_ref()
            .ok()
            .and_then(|message| message.as_ref())
            .map(control_kind),
        "ACP live runner rejected an unexpected mailbox message"
    );
}
