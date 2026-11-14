use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::projection::TurnTerminalReasonProjection;
use sage_acp_protocol::lifecycle::TurnTerminalReason;

use super::{AdapterStoreOwner, StoreError, TurnMutationCommand, history::TurnRow};

const MESSAGE_LIMIT: u64 = 8192;

pub(super) struct TerminalReasonOwner<'a> {
    project_id: &'a str,
    chat_id: &'a str,
    session_id: &'a str,
    turn_id: &'a str,
    runtime_epoch: i64,
}

impl<'a> From<&'a TurnMutationCommand> for TerminalReasonOwner<'a> {
    fn from(value: &'a TurnMutationCommand) -> Self {
        Self {
            project_id: value.project_id.as_str(),
            chat_id: value.chat_id.as_str(),
            session_id: value.session_id.as_str(),
            turn_id: value.turn_id.as_str(),
            runtime_epoch: value.runtime_epoch.get() as i64,
        }
    }
}

impl<'a> From<&'a AdapterStoreOwner> for TerminalReasonOwner<'a> {
    fn from(value: &'a AdapterStoreOwner) -> Self {
        Self {
            project_id: value.project_id.as_str(),
            chat_id: value.chat_id.as_str(),
            session_id: value.session_id.as_str(),
            turn_id: value.turn_id.as_str(),
            runtime_epoch: value.runtime_epoch.get() as i64,
        }
    }
}

fn reason_text(reason: TurnTerminalReason) -> &'static str {
    match reason {
        TurnTerminalReason::AuthRequired => "authRequired",
        TurnTerminalReason::HarnessExited => "harnessExited",
        TurnTerminalReason::ProtocolError => "protocolError",
        TurnTerminalReason::CancelledByUser => "cancelledByUser",
        TurnTerminalReason::RefusedVersion => "refusedVersion",
        TurnTerminalReason::Timeout => "timeout",
        TurnTerminalReason::Other => "other",
    }
}

fn parse_reason(value: &str) -> Result<TurnTerminalReason, StoreError> {
    match value {
        "authRequired" => Ok(TurnTerminalReason::AuthRequired),
        "harnessExited" => Ok(TurnTerminalReason::HarnessExited),
        "protocolError" => Ok(TurnTerminalReason::ProtocolError),
        "cancelledByUser" => Ok(TurnTerminalReason::CancelledByUser),
        "refusedVersion" => Ok(TurnTerminalReason::RefusedVersion),
        "timeout" => Ok(TurnTerminalReason::Timeout),
        "other" => Ok(TurnTerminalReason::Other),
        _ => Err(StoreError::Corrupt("turn terminal reason")),
    }
}

pub(super) fn record_in(
    transaction: &Transaction<'_>,
    owner: TerminalReasonOwner<'_>,
    reason: TurnTerminalReason,
    message: Option<&str>,
    remediation: Option<&str>,
    now_millis: i64,
) -> Result<bool, StoreError> {
    bounded(message, "turnTerminalReasonMessage")?;
    bounded(remediation, "turnTerminalReasonRemediation")?;
    let changed = transaction.execute(
        "INSERT OR IGNORE INTO acp_turn_terminal_reasons
         (project_id,chat_id,session_id,turn_id,runtime_epoch,kind,message,remediation,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            owner.project_id,
            owner.chat_id,
            owner.session_id,
            owner.turn_id,
            owner.runtime_epoch,
            reason_text(reason),
            message,
            remediation,
            now_millis
        ],
    )?;
    Ok(changed == 1)
}

fn bounded(value: Option<&str>, resource: &'static str) -> Result<(), StoreError> {
    if value.is_some_and(|value| value.is_empty() || value.len() as u64 > MESSAGE_LIMIT) {
        return Err(StoreError::Capacity {
            resource,
            limit: MESSAGE_LIMIT,
        });
    }
    Ok(())
}

pub(super) fn load(
    connection: &rusqlite::Connection,
    turn: &TurnRow,
) -> Result<Option<TurnTerminalReasonProjection>, StoreError> {
    let row: Option<(String, Option<String>, Option<String>)> = connection
        .query_row(
            "SELECT kind,message,remediation FROM acp_turn_terminal_reasons
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5",
            params![
                turn.project_id,
                turn.chat_id,
                turn.session_id,
                turn.id,
                turn.runtime_epoch
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    row.map(|(kind, message, remediation)| {
        Ok(TurnTerminalReasonProjection {
            kind: parse_reason(&kind)?,
            message,
            remediation,
        })
    })
    .transpose()
}
