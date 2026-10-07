use rusqlite::Transaction;

use crate::acp::{lifecycle::TurnTerminalReason, store::StoreError};

use super::AdapterStoreOwner;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnTerminalReasonCommand {
    pub reason: TurnTerminalReason,
    pub message: Option<String>,
    pub remediation: Option<String>,
    pub now_millis: i64,
}

pub(super) fn apply(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &TurnTerminalReasonCommand,
) -> Result<bool, StoreError> {
    crate::acp::store::terminal_reason::record_in(
        transaction,
        owner.into(),
        command.reason,
        command.message.as_deref(),
        command.remediation.as_deref(),
        command.now_millis,
    )
}
