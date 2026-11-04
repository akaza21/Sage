use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    acp::{
        contract::{EntityRevision, SageItemId, SageTurnId},
        limits::LIMITS,
    },
    persistence::Database,
};

use super::{
    MutationDisposition, PrepareTurnCommand, PreparedTurn, StoreError, TurnMutation,
    TurnMutationCommand,
    facts::{
        append_fact_payload, check_add, entity_revision_from_i64, hex_digest,
        next_projection_revision, projection_from_i64,
    },
};

impl Database {
    #[cfg(test)]
    pub(crate) fn acp_prepare_turn(
        &self,
        command: PrepareTurnCommand,
    ) -> Result<PreparedTurn, StoreError> {
        let mut connection = self
            .write_connection()
            .map_err(|_| StoreError::Corrupt("storage recovery required"))?;
        prepare_turn(&mut connection, command)
    }
}

pub(crate) fn lookup_prepared_turn(
    connection: &rusqlite::Connection,
    command: &PrepareTurnCommand,
) -> Result<Option<PreparedTurn>, StoreError> {
    let payload = user_item_payload(&command.prompt.user_payload())?;
    duplicate_turn(connection, command, &hex_digest(payload.as_bytes()))
}

pub(crate) fn lookup_active_turn(
    connection: &rusqlite::Connection,
    command: &TurnMutationCommand,
) -> Result<Option<TurnMutation>, StoreError> {
    let row = connection
        .query_row(
            "SELECT t.state,t.delivery_state,t.revision,c.projection_revision
             FROM turns t JOIN chats c ON c.id=t.chat_id AND c.project_id=t.project_id
             WHERE t.id=?1 AND t.project_id=?2 AND t.chat_id=?3 AND t.session_id=?4
               AND t.runtime_epoch=?5 LIMIT 1",
            params![
                command.turn_id.as_str(),
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    row.map(|(state, delivery, revision, projection)| {
        Ok(TurnMutation {
            state: super::send::parse_turn_state(&state)?,
            delivery_state: super::send::parse_delivery_state(&delivery)?,
            revision: entity_revision_from_i64(revision)?,
            projection_revision: projection_from_i64(projection)?,
            disposition: MutationDisposition::Duplicate,
        })
    })
    .transpose()
}

#[cfg(test)]
pub(crate) fn prepare_turn(
    connection: &mut rusqlite::Connection,
    command: PrepareTurnCommand,
) -> Result<PreparedTurn, StoreError> {
    let transaction = connection.transaction()?;
    let prepared = prepare_turn_in(&transaction, command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(prepared)
}

pub(super) fn prepare_turn_in(
    transaction: &Transaction<'_>,
    command: PrepareTurnCommand,
) -> Result<PreparedTurn, StoreError> {
    let user_payload = command.prompt.user_payload();
    let item_payload = user_item_payload(&user_payload)?;
    let item_bytes = i64::try_from(item_payload.len()).map_err(|_| StoreError::PayloadTooLarge)?;
    if item_bytes > LIMITS.wire.normalized_item_bytes as i64 {
        return Err(StoreError::PayloadTooLarge);
    }
    let item_digest = hex_digest(item_payload.as_bytes());
    if let Some(existing) = duplicate_turn(transaction, &command, &item_digest)? {
        return Ok(existing);
    }
    validate_prepare_owner(transaction, &command)?;
    ensure_turn_capacity(transaction, &command, item_bytes)?;
    let turn_id = SageTurnId::new();
    let item_id = SageItemId::new();
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM turns WHERE session_id=?1",
        [command.session_id.as_str()],
        |row| row.get(0),
    )?;
    if ordinal < 0 {
        return Err(StoreError::CounterExhausted);
    }
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    crate::chat_lifecycle::auto_title_first_prompt_in(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        user_payload["text"].as_str().unwrap_or_default(),
        command.now_millis,
    )?;
    transaction.execute(
        "INSERT INTO turns (id,project_id,chat_id,session_id,runtime_epoch,ordinal,
         client_intent_id,state,delivery_state,revision,created_at,updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,'queued','notWritten',0,?8,?8)",
        params![
            turn_id.as_str(),
            command.project_id.as_str(),
            command.chat_id.as_str(),
            command.session_id.as_str(),
            command.runtime_epoch.get() as i64,
            ordinal,
            command.client_intent_id.as_str(),
            command.now_millis
        ],
    )?;
    transaction.execute(
        "INSERT INTO items (id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
         origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,
         payload_digest,revision,created_at,updated_at,terminal_at)
         VALUES (?1,?2,?3,?4,?5,?6,0,'primary',NULL,'text','completed',NULL,NULL,?7,?8,?9,0,?10,?10,?10)",
        params![item_id.as_str(),command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),turn_id.as_str(),command.runtime_epoch.get() as i64,item_payload,item_bytes,item_digest,command.now_millis],
    )?;
    ensure_one(transaction.execute(
        "UPDATE acp_session_usage SET turn_count=turn_count+1,item_count=item_count+1,
         item_bytes=item_bytes+?2,payload_bytes=payload_bytes+?2,revision=revision+1 WHERE session_id=?1",
        params![command.session_id.as_str(),item_bytes],
    )?, "session usage")?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_project_usage SET turn_count=turn_count+1,item_count=item_count+1,
         payload_bytes=payload_bytes+?2,revision=revision+1 WHERE project_id=?1",
            params![command.project_id.as_str(), item_bytes],
        )?,
        "project usage",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE acp_store_usage SET turn_count=turn_count+1,item_count=item_count+1,
         item_bytes=item_bytes+?1,shared_payload_bytes=shared_payload_bytes+?1,
         revision=revision+1 WHERE singleton=1",
            [item_bytes],
        )?,
        "global usage",
    )?;
    let fact_payload = serde_json::to_string(&serde_json::json!({
        "kind":"sendIntent", "entityId":turn_id.as_str(), "state":"queued"
    }))
    .map_err(|_| StoreError::Corrupt("send intent serialization"))?;
    append_fact_payload(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        Some(turn_id.as_str()),
        "sendIntent",
        &fact_payload,
        projection,
        command.now_millis,
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='active',revision=revision+1,projection_revision=?2,
         updated_at=?3 WHERE id=?1",
            params![
                command.session_id.as_str(),
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "session",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE chats SET projection_revision=?3,updated_at=?4 WHERE project_id=?1 AND id=?2",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                projection.get() as i64,
                command.now_millis
            ],
        )?,
        "chat",
    )?;
    Ok(PreparedTurn {
        turn_id,
        item_id,
        turn_revision: EntityRevision::ZERO,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

fn validate_prepare_owner(
    transaction: &Transaction<'_>,
    command: &PrepareTurnCommand,
) -> Result<(), StoreError> {
    let revision: i64 = transaction
        .query_row(
            "SELECT projection_revision FROM chats WHERE project_id=?1 AND id=?2",
            params![command.project_id.as_str(), command.chat_id.as_str()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    if projection_from_i64(revision)? != command.expected_projection_revision {
        return Err(StoreError::Conflict {
            resource: "projectionRevision",
        });
    }
    let owner: Option<(String, String)> = transaction
        .query_row(
            "SELECT s.state,s.residency FROM agent_sessions s
         JOIN chat_session_heads h ON h.session_id=s.id
         JOIN session_runtime_epochs e ON e.session_id=s.id AND e.state='running'
         WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match owner {
        Some((state, residency))
            if matches!(state.as_str(), "ready" | "active") && residency == "hot" =>
        {
            Ok(())
        }
        _ => Err(StoreError::Conflict {
            resource: "sessionEpoch",
        }),
    }
}

fn duplicate_turn(
    connection: &rusqlite::Connection,
    command: &PrepareTurnCommand,
    digest: &str,
) -> Result<Option<PreparedTurn>, StoreError> {
    let existing: Option<(String,i64,i64,String,String,String)> = connection.query_row(
        "SELECT t.id,t.revision,t.runtime_epoch,i.id,t.project_id,t.chat_id FROM turns t
         JOIN items i ON i.turn_id=t.id AND i.ordinal=0 WHERE t.session_id=?1 AND t.client_intent_id=?2",
        params![command.session_id.as_str(),command.client_intent_id.as_str()],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?)),
    ).optional()?;
    let Some((turn, revision, epoch, item, project, chat)) = existing else {
        return Ok(None);
    };
    let stored_digest: String = connection.query_row(
        "SELECT payload_digest FROM items WHERE id=?1",
        [&item],
        |row| row.get(0),
    )?;
    if epoch != command.runtime_epoch.get() as i64
        || stored_digest != digest
        || project != command.project_id.as_str()
        || chat != command.chat_id.as_str()
    {
        return Err(StoreError::Conflict {
            resource: "clientIntent",
        });
    }
    let projection: i64 = connection.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE id=?1",
        [command.session_id.as_str()],
        |row| row.get(0),
    )?;
    Ok(Some(PreparedTurn {
        turn_id: SageTurnId::parse(turn).map_err(|_| StoreError::Corrupt("turn id"))?,
        item_id: SageItemId::parse(item).map_err(|_| StoreError::Corrupt("item id"))?,
        turn_revision: entity_revision_from_i64(revision)?,
        projection_revision: projection_from_i64(projection)?,
        disposition: MutationDisposition::Duplicate,
    }))
}

fn user_item_payload(payload: &serde_json::Value) -> Result<String, StoreError> {
    serde_json::to_string(&serde_json::json!({"kind":"text","payload":payload}))
        .map_err(|_| StoreError::Corrupt("user item serialization"))
}

fn ensure_turn_capacity(
    transaction: &Transaction<'_>,
    command: &PrepareTurnCommand,
    item_bytes: i64,
) -> Result<(), StoreError> {
    let session:(i64,i64,i64,i64)=transaction.query_row("SELECT turn_count,item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id=?1",[command.session_id.as_str()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or(StoreError::Corrupt("missing session usage"))?;
    let project: (i64, i64) = transaction
        .query_row(
            "SELECT turn_count,item_count FROM acp_project_usage WHERE project_id=?1",
            [command.project_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Corrupt("missing project usage"))?;
    let global:(i64,i64,i64,i64)=transaction.query_row("SELECT turn_count,item_count,item_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?.ok_or(StoreError::Corrupt("missing global usage"))?;
    check_add(
        session.0,
        1,
        LIMITS.durable.turns_per_session,
        "sessionTurns",
    )?;
    check_add(
        session.1,
        1,
        LIMITS.durable.items_per_session,
        "sessionItems",
    )?;
    check_add(
        session.2,
        item_bytes,
        LIMITS.durable.item_bytes_per_session,
        "sessionItemBytes",
    )?;
    check_add(
        session.3,
        item_bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "sessionPayload",
    )?;
    check_add(
        project.0,
        1,
        LIMITS.durable.turns_per_project,
        "projectTurns",
    )?;
    check_add(global.0, 1, LIMITS.durable.turns_global, "globalTurns")?;
    check_add(global.1, 1, LIMITS.durable.items_global, "globalItems")?;
    check_add(
        global.2,
        item_bytes,
        LIMITS.durable.item_bytes_global,
        "globalItemBytes",
    )?;
    check_add(
        global.3,
        item_bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "globalPayload",
    )
}

fn ensure_one(changed: usize, resource: &'static str) -> Result<(), StoreError> {
    if changed == 1 {
        Ok(())
    } else {
        Err(StoreError::Corrupt(resource))
    }
}
