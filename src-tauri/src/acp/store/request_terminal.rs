use rusqlite::{OptionalExtension, Transaction, params};

use super::{
    EntityMutation, RequestStateCommand, StoreError,
    facts::{entity_revision_from_i64, next_projection_revision, publish_projection},
    state::{parse_request_state, request_state_text},
    terminal::{append_entity_fact, applied, duplicate, ensure_one},
};

pub(crate) fn apply_request_state(
    connection: &mut rusqlite::Connection,
    command: RequestStateCommand,
) -> Result<EntityMutation, StoreError> {
    let transaction = connection.transaction()?;
    let row: (String, i64, String, String) = transaction
        .query_row(
            "SELECT state,revision,external_request_id,normalized_payload_json FROM server_requests
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
               AND runtime_epoch=?5 AND id=?6",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.turn_id.as_str(),
                command.runtime_epoch.get() as i64,
                command.request_id.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "request",
        })?;
    let current = parse_request_state(&row.0)?;
    if current == command.state {
        if current.is_terminal() {
            verify_terminal_item(&transaction, &command, &row.2, &row.3)?;
        }
        return duplicate(&transaction, command.session_id.as_str(), row.1);
    }
    if current.is_terminal() {
        return Err(StoreError::TerminalConflict {
            resource: "request",
        });
    }
    if !current.can_transition_to(command.state) {
        return Err(StoreError::InvalidTransition {
            resource: "request",
        });
    }
    let revision = entity_revision_from_i64(row.1)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE server_requests SET state=?2,revision=?3,updated_at=?4,resolved_at=?5
             WHERE id=?1",
            params![
                command.request_id.as_str(),
                request_state_text(command.state),
                revision.get() as i64,
                command.now_millis,
                command.state.is_terminal().then_some(command.now_millis)
            ],
        )?,
        "request",
    )?;
    if command.state.is_terminal() {
        terminalize_permission_item(&transaction, &command, &row.2, &row.3)?;
    }
    append_entity_fact(
        &transaction,
        &command.project_id,
        &command.chat_id,
        &command.session_id,
        Some(&command.turn_id),
        "requestState",
        command.request_id.as_str(),
        request_state_text(command.state),
        projection,
        command.now_millis,
    )?;
    publish_projection(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(applied(revision, projection))
}

fn terminalize_permission_item(
    transaction: &Transaction<'_>,
    command: &RequestStateCommand,
    external_id: &str,
    normalized_payload_json: &str,
) -> Result<(), StoreError> {
    let tool_state = match command.state {
        crate::acp::lifecycle::ServerRequestState::ResolvedAllowed => "completed",
        crate::acp::lifecycle::ServerRequestState::ResolvedDeclined => "declined",
        crate::acp::lifecycle::ServerRequestState::Cancelled => "cancelled",
        _ => {
            return Err(StoreError::InvalidTransition {
                resource: "request",
            });
        }
    };
    ensure_one(
        transaction.execute(
            "UPDATE items SET lifecycle='completed',tool_state=?9,revision=revision+1,
             updated_at=?7,terminal_at=?7 WHERE project_id=?1 AND chat_id=?2 AND session_id=?3
             AND turn_id=?4 AND runtime_epoch=?5 AND kind='permissionRequest'
             AND external_item_id=?6 AND payload_json=?8 AND lifecycle IN ('started','streaming')
             AND tool_state='waitingForApproval'",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.turn_id.as_str(),
                command.runtime_epoch.get() as i64,
                external_id,
                command.now_millis,
                normalized_payload_json,
                tool_state
            ],
        )?,
        "permission item",
    )
}

fn verify_terminal_item(
    transaction: &Transaction<'_>,
    command: &RequestStateCommand,
    external_id: &str,
    normalized_payload_json: &str,
) -> Result<(), StoreError> {
    let tool_state = match command.state {
        crate::acp::lifecycle::ServerRequestState::ResolvedAllowed => "completed",
        crate::acp::lifecycle::ServerRequestState::ResolvedDeclined => "declined",
        crate::acp::lifecycle::ServerRequestState::Cancelled => "cancelled",
        _ => {
            return Err(StoreError::InvalidTransition {
                resource: "request",
            });
        }
    };
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM items WHERE project_id=?1 AND chat_id=?2 AND session_id=?3
         AND turn_id=?4 AND runtime_epoch=?5 AND kind='permissionRequest'
         AND external_item_id=?6 AND payload_json=?7 AND lifecycle='completed'
         AND tool_state=?8",
        params![
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.turn_id.as_str(),
            command.runtime_epoch.get() as i64,
            external_id,
            normalized_payload_json,
            tool_state
        ],
        |row| row.get(0),
    )?;
    if count == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt("terminal permission item"))
    }
}
