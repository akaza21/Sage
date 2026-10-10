use super::*;
use crate::harnesses::SIMULATOR_ID;

pub(crate) fn seed_legacy_welcome(
    transaction: &Transaction<'_>,
    project_id: &str,
    tab_id: &str,
    session_id: &str,
    now: i64,
    initial_payload: &str,
    fact_kind: &str,
) -> AppResult<()> {
    transaction.execute(
        "UPDATE chats SET projection_revision=1 WHERE id=?1",
        [tab_id],
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
    let fact_payload = turn_fact_payload(1);
    let fact_bytes = fact_payload.len() as i64;
    transaction.execute(
        "INSERT INTO session_facts(
           project_id,chat_id,session_id,sequence,turn_id,kind,payload_json,payload_bytes,
           payload_digest,projection_revision,created_at
         ) VALUES (?1,?2,?3,1,?4,?5,?6,?7,?8,1,?9)",
        params![
            project_id,
            tab_id,
            session_id,
            turn_id,
            fact_kind,
            fact_payload,
            fact_bytes,
            digest(fact_payload.as_bytes()),
            now
        ],
    )?;
    let item_bytes = initial_payload.len() as i64;
    transaction.execute(
        "INSERT INTO acp_session_usage(
           project_id,chat_id,session_id,turn_count,item_count,item_bytes,
           fact_count,fact_bytes,payload_bytes,revision
         ) VALUES (?1,?2,?3,1,1,?4,1,?5,?4+?5,0)",
        params![project_id, tab_id, session_id, item_bytes, fact_bytes],
    )?;
    transaction.execute(
        "UPDATE acp_project_usage SET session_count=session_count+1,turn_count=turn_count+1,
         item_count=item_count+1,fact_count=fact_count+1,payload_bytes=payload_bytes+?2
         WHERE project_id=?1",
        params![project_id, item_bytes + fact_bytes],
    )?;
    let changed = transaction.execute(
        "UPDATE acp_store_usage SET
           session_count=session_count+1,turn_count=turn_count+1,
           item_count=item_count+1,item_bytes=item_bytes+?1,fact_count=fact_count+1,
           fact_bytes=fact_bytes+?2,shared_payload_bytes=shared_payload_bytes+?1+?2,
           revision=revision+1 WHERE singleton=1",
        params![item_bytes, fact_bytes],
    )?;
    require_one(changed, "global ACP usage ledger is absent")
}
