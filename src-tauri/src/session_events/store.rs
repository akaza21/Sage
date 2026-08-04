mod capacity;
mod session_owner;

use rusqlite::{Transaction, params};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    harnesses::SIMULATOR_ID,
};

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

pub(super) fn seed_simulator_chat(
    transaction: &Transaction<'_>,
    project_id: &str,
    tab_id: &str,
    session_id: &str,
    now: i64,
    initial_payload: &str,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
         VALUES (?1,?2,0,'Chat: New project',1,?3,?3)",
        params![tab_id, project_id, now],
    )?;
    transaction.execute(
        "INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at) VALUES (?1,?2,?2,?3)",
        params![project_id, tab_id, now],
    )?;
    let binding_digest = digest(format!("sage-simulator-binding-v1\0{SIMULATOR_ID}").as_bytes());
    transaction.execute(
        "INSERT INTO agent_sessions(
           id,project_id,chat_id,ordinal,binding_id,binding_digest,model_id,
           external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,
           created_at,updated_at,terminal_at
         ) VALUES (?1,?2,?3,0,?4,?5,'sim-fast',?6,?7,'closed','cold',0,0,1,?8,?8,?8)",
        params![
            session_id,
            project_id,
            tab_id,
            SIMULATOR_ID,
            binding_digest,
            format!("sage-simulator:{session_id}"),
            SIMULATOR_CAPABILITIES,
            now
        ],
    )?;
    insert_exited_epoch(transaction, project_id, tab_id, session_id, 1, now)?;
    let turn_id = Uuid::new_v4().to_string();
    let turn = CompletedTurn {
        owner: Owner {
            project: project_id,
            chat: tab_id,
            session: session_id,
        },
        id: &turn_id,
        runtime_epoch: 1,
        now,
    };
    insert_completed_turn(transaction, &turn, 0)?;
    insert_text_item(transaction, &turn, 0, initial_payload)?;
    let fact_bytes = insert_turn_fact(transaction, &turn, 1, 1, 1)?;
    let item_bytes = initial_payload.len() as i64;
    transaction.execute(
        "INSERT INTO acp_session_usage(
           project_id,chat_id,session_id,turn_count,item_count,item_bytes,
           fact_count,fact_bytes,payload_bytes,revision
         ) VALUES (?1,?2,?3,1,1,?4,1,?5,?4+?5,0)",
        params![project_id, tab_id, session_id, item_bytes, fact_bytes],
    )?;
    transaction.execute(
        "INSERT INTO acp_project_usage(
           project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision
         ) VALUES (?1,1,1,1,1,1,?2,0)",
        params![project_id, item_bytes + fact_bytes],
    )?;
    let changed = transaction.execute(
        "UPDATE acp_store_usage SET
           chat_count=chat_count+1,session_count=session_count+1,turn_count=turn_count+1,
           item_count=item_count+1,item_bytes=item_bytes+?1,fact_count=fact_count+1,
           fact_bytes=fact_bytes+?2,shared_payload_bytes=shared_payload_bytes+?1+?2,
           revision=revision+1 WHERE singleton=1",
        params![item_bytes, fact_bytes],
    )?;
    require_one(changed, "global ACP usage ledger is absent")
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
