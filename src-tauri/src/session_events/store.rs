mod capacity;
mod session_owner;

#[cfg(test)]
mod legacy_welcome_fixture;
#[cfg(test)]
pub(crate) use legacy_welcome_fixture::seed_legacy_welcome;

use rusqlite::{Transaction, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use self::{
    capacity::Usage,
    session_owner::{next_chat_projection, resolve_or_create},
};

const SIMULATOR_CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"sage-simulator-v1\",\"newSession\":false,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":false,\"prompt\":true,\"cancel\":false,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":2,\"configCount\":0}";
#[derive(Clone, Copy)]
struct Owner<'a> {
    project: &'a str,
    chat: &'a str,
    session: &'a str,
}

#[derive(Clone, Copy)]
struct CompletedTurn<'a> {
    owner: Owner<'a>,
    id: &'a str,
    runtime_epoch: i64,
    now: i64,
}

pub(super) fn append_turn(
    transaction: &Transaction<'_>,
    chat_id: &str,
    model_id: &str,
    payloads: &[String; 2],
    now: i64,
    after_first_insert: impl FnOnce(),
) -> AppResult<String> {
    let owner = resolve_or_create(transaction, chat_id, model_id, now)?;
    let usage = capacity::load(transaction, &owner.project, &owner.session)?;
    let item_bytes = payloads.iter().map(String::len).sum::<usize>() as i64;
    let fact_payload = turn_fact_payload(2);
    capacity::ensure(&usage, item_bytes, fact_payload.len() as i64)?;
    let turn_id = Uuid::new_v4().to_string();
    let runtime_epoch = usage.session_turns + 1;
    insert_exited_epoch(
        transaction,
        &owner.project,
        &owner.chat,
        &owner.session,
        runtime_epoch,
        now,
    )?;
    let turn = CompletedTurn {
        owner: Owner {
            project: &owner.project,
            chat: &owner.chat,
            session: &owner.session,
        },
        id: &turn_id,
        runtime_epoch,
        now,
    };
    let user_payload: serde_json::Value = serde_json::from_str(&payloads[0])
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    crate::chat_lifecycle::auto_title_first_prompt_in(
        transaction,
        &owner.project,
        &owner.chat,
        user_payload["text"].as_str().unwrap_or_default(),
        now,
    )?;
    insert_completed_turn(transaction, &turn, usage.session_turns)?;
    insert_text_item(transaction, &turn, 0, &payloads[0])?;
    after_first_insert();
    insert_text_item(transaction, &turn, 1, &payloads[1])?;
    let fact_sequence = usage.session_facts + 1;
    let projection_revision = next_chat_projection(transaction, &owner.project, &owner.chat)?;
    let fact_bytes = insert_turn_fact(transaction, &turn, fact_sequence, projection_revision, 2)?;
    update_ledgers(
        transaction,
        &owner.project,
        &owner.session,
        &usage,
        item_bytes,
        fact_bytes,
    )?;
    require_one(
        transaction.execute(
            "UPDATE chats SET projection_revision=?3,updated_at=?4
             WHERE project_id=?1 AND id=?2 AND projection_revision=?3-1",
            params![owner.project, owner.chat, projection_revision, now],
        )?,
        "simulator Chat disappeared during append",
    )?;
    require_one(
        transaction.execute(
            "UPDATE agent_sessions
             SET projection_revision=?2,revision=revision+1,updated_at=?3 WHERE id=?1",
            params![owner.session, projection_revision, now],
        )?,
        "simulator session disappeared during append",
    )?;
    Ok(owner.session)
}

fn insert_exited_epoch(
    transaction: &Transaction<'_>,
    project: &str,
    chat: &str,
    session: &str,
    runtime_epoch: i64,
    now: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO session_runtime_epochs(
           project_id,chat_id,session_id,runtime_epoch,state,started_at,terminal_at,exit_kind
         ) VALUES (?1,?2,?3,?4,'exited',?5,?5,'simulatorSynchronous')",
        params![project, chat, session, runtime_epoch, now],
    )?;
    Ok(())
}

fn insert_completed_turn(
    transaction: &Transaction<'_>,
    turn: &CompletedTurn<'_>,
    ordinal: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO turns(
           id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
           state,delivery_state,revision,created_at,updated_at,terminal_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,'completed','correlated',0,?8,?8,?8)",
        params![
            turn.id,
            turn.owner.project,
            turn.owner.chat,
            turn.owner.session,
            turn.runtime_epoch,
            ordinal,
            format!("simulator-{}", turn.id),
            turn.now
        ],
    )?;
    Ok(())
}

fn insert_text_item(
    transaction: &Transaction<'_>,
    turn: &CompletedTurn<'_>,
    ordinal: i64,
    payload: &str,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO items(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,
           payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,'primary','text','completed',?8,?9,?10,0,?11,?11,?11)",
        params![
            Uuid::new_v4().to_string(),
            turn.owner.project,
            turn.owner.chat,
            turn.owner.session,
            turn.id,
            turn.runtime_epoch,
            ordinal,
            payload,
            payload.len() as i64,
            digest(payload.as_bytes()),
            turn.now
        ],
    )?;
    Ok(())
}

fn insert_turn_fact(
    transaction: &Transaction<'_>,
    turn: &CompletedTurn<'_>,
    sequence: i64,
    projection_revision: i64,
    item_count: i64,
) -> AppResult<i64> {
    let payload = turn_fact_payload(item_count);
    transaction.execute(
        "INSERT INTO session_facts(
           project_id,chat_id,session_id,sequence,turn_id,kind,payload_json,payload_bytes,
           payload_digest,projection_revision,created_at
         ) VALUES (?1,?2,?3,?4,?5,'simulatorTurnCommitted',?6,?7,?8,?9,?10)",
        params![
            turn.owner.project,
            turn.owner.chat,
            turn.owner.session,
            sequence,
            turn.id,
            payload,
            payload.len() as i64,
            digest(payload.as_bytes()),
            projection_revision,
            turn.now
        ],
    )?;
    Ok(payload.len() as i64)
}

fn update_ledgers(
    transaction: &Transaction<'_>,
    project: &str,
    session: &str,
    usage: &Usage,
    item_bytes: i64,
    fact_bytes: i64,
) -> AppResult<()> {
    let payload_bytes = item_bytes + fact_bytes;
    require_one(
        transaction.execute(
            "UPDATE acp_session_usage SET turn_count=turn_count+1,item_count=item_count+2,
           item_bytes=item_bytes+?2,fact_count=fact_count+1,fact_bytes=fact_bytes+?3,
           payload_bytes=payload_bytes+?2+?3,revision=revision+1
         WHERE session_id=?1 AND turn_count=?4 AND item_count=?5 AND payload_bytes=?6",
            params![
                session,
                item_bytes,
                fact_bytes,
                usage.session_turns,
                usage.session_items,
                usage.session_payload_bytes
            ],
        )?,
        "simulator session usage ledger changed",
    )?;
    require_one(
        transaction.execute(
            "UPDATE acp_project_usage SET turn_count=turn_count+1,item_count=item_count+2,
           fact_count=fact_count+1,payload_bytes=payload_bytes+?2,revision=revision+1
         WHERE project_id=?1 AND turn_count=?3",
            params![project, payload_bytes, usage.project_turns],
        )?,
        "simulator project usage ledger changed",
    )?;
    require_one(
        transaction.execute(
            "UPDATE acp_store_usage SET turn_count=turn_count+1,item_count=item_count+2,
           item_bytes=item_bytes+?1,fact_count=fact_count+1,fact_bytes=fact_bytes+?2,
           shared_payload_bytes=shared_payload_bytes+?1+?2,revision=revision+1
         WHERE singleton=1 AND turn_count=?3 AND item_count=?4 AND shared_payload_bytes=?5",
            params![
                item_bytes,
                fact_bytes,
                usage.store_turns,
                usage.store_items,
                usage.store_payload_bytes
            ],
        )?,
        "global ACP usage ledger changed",
    )
}

fn turn_fact_payload(item_count: i64) -> String {
    json!({ "itemCount": item_count }).to_string()
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn require_one(changed: usize, message: &str) -> AppResult<()> {
    if changed != 1 {
        return Err(AppError::InvalidRequest(message.into()));
    }
    Ok(())
}
