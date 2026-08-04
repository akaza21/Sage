use rusqlite::{Transaction, params};

use crate::acp::{
    contract::SageItemId,
    lifecycle::ServerRequestState,
    runtime::permissions::{DeclineCommit, PermissionError, PermissionIntent, PermissionJournal},
};

use super::{
    EntityMutation, MutationDisposition, RequestStateCommand, StoreError, apply_request_state,
    facts::{append_fact_payload, next_projection_revision, publish_projection},
};

pub(crate) struct DenyOnlyPermissionJournal<'a> {
    connection: &'a mut rusqlite::Connection,
    now_millis: i64,
}

impl<'a> DenyOnlyPermissionJournal<'a> {
    pub(crate) fn new(connection: &'a mut rusqlite::Connection, now_millis: i64) -> Self {
        Self {
            connection,
            now_millis,
        }
    }
}

pub(crate) fn receive_permission_request(
    connection: &mut rusqlite::Connection,
    intent: &PermissionIntent,
    now_millis: i64,
) -> Result<EntityMutation, StoreError> {
    let transaction = connection.transaction()?;
    let result = receive_permission_request_in(&transaction, intent, now_millis)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(result)
}

pub(super) fn receive_permission_request_in(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    now_millis: i64,
) -> Result<EntityMutation, StoreError> {
    super::request_identity::validate_payload(intent)?;
    if let Some(existing) = super::request_identity::find(transaction, intent)? {
        super::request_identity::verify(&existing, intent)?;
        let (lifecycle, tool_state) = if existing.state == "resolvedDeclined" {
            ("completed", "declined")
        } else {
            ("started", "waitingForApproval")
        };
        super::permission_item::verify_pair(transaction, intent, lifecycle, tool_state)?;
        return duplicate(transaction, intent, existing.revision);
    }
    super::request_identity::verify_owner(transaction, intent)?;
    let payload_bytes = i64::try_from(intent.normalized_payload_json.len())
        .map_err(|_| StoreError::PayloadTooLarge)?;
    super::permission_item::ensure_request_capacity(transaction, intent, payload_bytes)?;
    super::permission_item::ensure_capacity(transaction, intent, payload_bytes)?;
    super::permission_item::ensure_absent(transaction, intent)?;
    let projection = next_projection_revision(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
    )?;
    let item_id = SageItemId::new();
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal),-1)+1 FROM items WHERE turn_id=?1",
        [intent.turn_id.as_str()],
        |row| row.get(0),
    )?;
    if ordinal < 0 {
        return Err(StoreError::CounterExhausted);
    }
    transaction.execute(
        "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
         origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,
         payload_digest,revision,created_at,updated_at,terminal_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,'primary',NULL,'permissionRequest','started',
         'waitingForApproval',?8,?9,?10,?11,0,?12,?12,NULL)",
        params![
            item_id.as_str(),
            intent.project_id.as_str(),
            intent.chat_id.as_str(),
            intent.session_id.as_str(),
            intent.turn_id.as_str(),
            intent.runtime_epoch.get() as i64,
            ordinal,
            intent.external_request_id.as_str(),
            intent.normalized_payload_json,
            payload_bytes,
            intent.normalized_payload_digest,
            now_millis
        ],
    )?;
    super::permission_item::update_usage(transaction, intent, payload_bytes)?;
    transaction.execute(
        "INSERT INTO server_requests(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
           external_request_id,kind,payload_digest_version,payload_digest,state,
           normalized_payload_json,revision,created_at,updated_at,resolved_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,?10,'received',?11,0,?12,?12,NULL)",
        params![
            intent.request_id.as_str(),
            intent.project_id.as_str(),
            intent.chat_id.as_str(),
            intent.session_id.as_str(),
            intent.turn_id.as_str(),
            intent.runtime_epoch.get() as i64,
            intent.binding_digest,
            intent.external_request_id.as_str(),
            intent.kind.as_str(),
            intent.payload_digest,
            intent.normalized_payload_json,
            now_millis
        ],
    )?;
    super::permission_item::append_fact(transaction, intent, &item_id, projection, now_millis)?;
    append_request_fact(transaction, intent, "received", projection, now_millis)?;
    publish_projection(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
        intent.session_id.as_str(),
        projection,
        now_millis,
    )?;
    Ok(EntityMutation {
        revision: crate::acp::contract::EntityRevision::ZERO,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

impl PermissionJournal for DenyOnlyPermissionJournal<'_> {
    fn commit_decline(
        &mut self,
        intent: &PermissionIntent,
    ) -> Result<DeclineCommit, PermissionError> {
        commit_decline(self.connection, intent, self.now_millis)
            .map_err(|_| PermissionError::Durable)
    }

    fn response_queued(&mut self, intent: &PermissionIntent) -> Result<(), PermissionError> {
        advance(
            self.connection,
            intent,
            ServerRequestState::ResponseQueued,
            self.now_millis,
        )
        .map(|_| ())
        .map_err(|_| PermissionError::Durable)
    }

    fn resolved_declined(&mut self, intent: &PermissionIntent) -> Result<(), PermissionError> {
        advance(
            self.connection,
            intent,
            ServerRequestState::ResolvedDeclined,
            self.now_millis,
        )
        .map(|_| ())
        .map_err(|_| PermissionError::Durable)
    }
}

fn commit_decline(
    connection: &mut rusqlite::Connection,
    intent: &PermissionIntent,
    now: i64,
) -> Result<DeclineCommit, StoreError> {
    let transaction = connection.transaction()?;
    let result = commit_decline_in(&transaction, intent, now)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(result)
}

pub(super) fn commit_decline_in(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    now: i64,
) -> Result<DeclineCommit, StoreError> {
    super::request_identity::validate_payload(intent)?;
    let existing =
        super::request_identity::find(transaction, intent)?.ok_or(StoreError::Conflict {
            resource: "request",
        })?;
    super::request_identity::verify(&existing, intent)?;
    if matches!(
        existing.state.as_str(),
        "declineCommitted" | "responseQueued" | "resolvedDeclined"
    ) {
        return Ok(DeclineCommit::IdenticalDuplicate);
    }
    if existing.state != "received" && existing.state != "declinePending" {
        return Err(StoreError::InvalidTransition {
            resource: "request",
        });
    }
    if existing.state == "received" {
        ensure_one(
            transaction.execute(
                "UPDATE server_requests SET state='declinePending',revision=revision+1,
                 updated_at=?2 WHERE id=?1",
                params![intent.request_id.as_str(), now],
            )?,
            "request",
        )?;
    }
    ensure_one(
        transaction.execute(
            "UPDATE server_requests SET state='declineCommitted',revision=revision+1,
             updated_at=?2 WHERE id=?1",
            params![intent.request_id.as_str(), now],
        )?,
        "request",
    )?;
    let projection = next_projection_revision(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
    )?;
    append_request_fact(transaction, intent, "declineCommitted", projection, now)?;
    publish_projection(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
        intent.session_id.as_str(),
        projection,
        now,
    )?;
    Ok(DeclineCommit::Applied)
}

fn advance(
    connection: &mut rusqlite::Connection,
    intent: &PermissionIntent,
    state: ServerRequestState,
    now: i64,
) -> Result<EntityMutation, StoreError> {
    let row = super::request_identity::find(connection, intent)?.ok_or(StoreError::Conflict {
        resource: "request",
    })?;
    super::request_identity::verify(&row, intent)?;
    apply_request_state(
        connection,
        RequestStateCommand {
            project_id: intent.project_id.clone(),
            chat_id: intent.chat_id.clone(),
            session_id: intent.session_id.clone(),
            turn_id: intent.turn_id.clone(),
            runtime_epoch: intent.runtime_epoch,
            request_id: intent.request_id.clone(),
            state,
            now_millis: now,
        },
    )
}

fn append_request_fact(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    state: &str,
    projection: crate::acp::contract::ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "requestState",
        "entityId": intent.request_id.as_str(),
        "state": state
    }))
    .map_err(|_| StoreError::Corrupt("request fact"))?;
    append_fact_payload(
        transaction,
        intent.project_id.as_str(),
        intent.chat_id.as_str(),
        intent.session_id.as_str(),
        Some(intent.turn_id.as_str()),
        "requestState",
        &payload,
        projection,
        now,
    )
}

fn duplicate(
    transaction: &Transaction<'_>,
    intent: &PermissionIntent,
    revision: i64,
) -> Result<EntityMutation, StoreError> {
    let projection: i64 = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [intent.session_id.as_str()],
        |r| r.get(0),
    )?;
    Ok(EntityMutation {
        revision: crate::acp::contract::EntityRevision::new(revision as u64)
            .map_err(|_| StoreError::Corrupt("request revision"))?,
        projection_revision: crate::acp::contract::ProjectionRevision::new(projection as u64)
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
        disposition: MutationDisposition::Duplicate,
    })
}
fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}
