use std::collections::BTreeMap;

use rusqlite::params_from_iter;

use crate::acp::projection::{ActivityProjection, ActivityRole};
use sage_acp_protocol::{
    contract::{EntityRevision, SageActivityId, SageItemId, SageTurnId},
    intent::ActivityProvenanceV1,
};
use sage_core::limits::LIMITS;

use super::{StoreError, history::TurnRow};

pub(super) fn retain_bounded_turn_prefix(
    connection: &rusqlite::Connection,
    turns: &mut Vec<TurnRow>,
    has_more: &mut bool,
) -> Result<(), StoreError> {
    if turns.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = turns.iter().map(|value| value.id.as_str()).collect();
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT turn_id,COUNT(*) FROM session_activities
         WHERE turn_id IN ({marks}) GROUP BY turn_id LIMIT 21"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut counts = BTreeMap::new();
    for row in statement.query_map(params_from_iter(ids.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (turn_id, count) = row?;
        if count < 0 || count > LIMITS.native_thread_projection.activity_groups_per_turn as i64 {
            return Err(StoreError::Corrupt("activity groups per turn"));
        }
        counts.insert(turn_id, count as usize);
    }
    let mut retained = 0_usize;
    let mut keep = turns.len();
    for (index, turn) in turns.iter().enumerate() {
        let next = retained
            .checked_add(counts.get(&turn.id).copied().unwrap_or(0))
            .ok_or(StoreError::Corrupt("activity page count"))?;
        if next > LIMITS.native_thread_projection.activity_nodes_per_page as usize {
            keep = index;
            *has_more = true;
            break;
        }
        retained = next;
    }
    turns.truncate(keep);
    Ok(())
}

pub(super) fn load_activities(
    connection: &rusqlite::Connection,
    turns: &[TurnRow],
) -> Result<BTreeMap<String, Vec<ActivityProjection>>, StoreError> {
    let mut grouped = BTreeMap::new();
    if turns.is_empty() {
        return Ok(grouped);
    }
    let ids: Vec<&str> = turns.iter().map(|value| value.id.as_str()).collect();
    let marks = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT id,turn_id,parent_activity_id,parent_item_id,role,lifecycle,depth,
                provenance_json,revision FROM session_activities WHERE turn_id IN ({marks})
         ORDER BY turn_id,observed_sequence LIMIT 129"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut seen = 0;
    for row in statement.query_map(params_from_iter(ids.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, i64>(8)?,
        ))
    })? {
        seen += 1;
        if seen > 128 {
            return Err(StoreError::Corrupt("activity page limit+1"));
        }
        let row = row?;
        let turn = row.1.clone();
        let provenance = serde_json::from_str::<ActivityProvenanceV1>(&row.7)
            .map_err(|_| StoreError::Corrupt("activity provenance"))?;
        provenance
            .validate_for_persistence()
            .map_err(|_| StoreError::Corrupt("activity provenance authority"))?;
        grouped
            .entry(turn.clone())
            .or_insert_with(Vec::new)
            .push(ActivityProjection {
                id: SageActivityId::parse(row.0).map_err(|_| StoreError::Corrupt("activity id"))?,
                turn_id: SageTurnId::parse(turn).map_err(|_| StoreError::Corrupt("turn id"))?,
                parent_activity_id: row
                    .2
                    .map(SageActivityId::parse)
                    .transpose()
                    .map_err(|_| StoreError::Corrupt("parent activity"))?,
                parent_item_id: SageItemId::parse(row.3)
                    .map_err(|_| StoreError::Corrupt("parent item"))?,
                role: parse_role(&row.4)?,
                lifecycle: super::state::parse_activity_lifecycle(&row.5)?,
                depth: u8::try_from(row.6).map_err(|_| StoreError::Corrupt("activity depth"))?,
                provenance,
                revision: EntityRevision::new(row.8 as u64)
                    .map_err(|_| StoreError::Corrupt("activity revision"))?,
            });
    }
    Ok(grouped)
}

fn parse_role(value: &str) -> Result<ActivityRole, StoreError> {
    match value {
        "root" => Ok(ActivityRole::Root),
        "fork" => Ok(ActivityRole::Fork),
        "child" => Ok(ActivityRole::Child),
        "unknown" => Ok(ActivityRole::Unknown),
        _ => Err(StoreError::Corrupt("activity role")),
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use crate::{
        acp::store::{facts::hex_digest, load_chat_page},
        persistence::Database,
    };
    use sage_acp_protocol::contract::{SageChatId, SageProjectId};

    const CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"pending\",\"newSession\":false,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":false,\"prompt\":false,\"cancel\":false,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":0,\"configCount\":0}";
    const PROVENANCE: &str = "{\"schemaVersion\":1,\"sourceKind\":\"collaboration\",\"externalKind\":\"subagent\",\"extensionSchemaId\":null,\"safeLabel\":\"Worker\"}";

    #[test]
    fn activity_page_returns_a_whole_turn_prefix_and_continuation_at_129() {
        let exact = seeded_page(&[32, 32, 32, 32]);
        assert_eq!(activity_count(&exact, None), (128, false));

        let plus_one = seeded_page(&[32, 32, 32, 32, 1]);
        let connection = plus_one.connection();
        let first = load_chat_page(
            &connection,
            SageProjectId::parse("project-a").unwrap(),
            SageChatId::parse("chat-a").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            first
                .turns
                .as_slice()
                .iter()
                .map(|turn| turn.activities.as_slice().len())
                .sum::<usize>(),
            97
        );
        assert!(first.next_cursor.is_some());
        let second = load_chat_page(
            &connection,
            first.project_id,
            first.chat_id,
            first.next_cursor,
        )
        .unwrap();
        assert_eq!(
            second
                .turns
                .as_slice()
                .iter()
                .map(|turn| turn.activities.as_slice().len())
                .sum::<usize>(),
            32
        );
        assert!(second.next_cursor.is_none());
    }

    fn activity_count(
        database: &Database,
        cursor: Option<sage_acp_protocol::intent::PageCursor>,
    ) -> (usize, bool) {
        let connection = database.connection();
        let page = load_chat_page(
            &connection,
            SageProjectId::parse("project-a").unwrap(),
            SageChatId::parse("chat-a").unwrap(),
            cursor,
        )
        .unwrap();
        (
            page.turns
                .as_slice()
                .iter()
                .map(|turn| turn.activities.as_slice().len())
                .sum(),
            page.next_cursor.is_some(),
        )
    }

    fn seeded_page(activity_counts: &[usize]) -> Database {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let connection = database.connection();
        connection
            .execute_batch(
                "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
                   VALUES ('project-a','Project',1,1,1);
                 INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                   VALUES ('root-a','project-a','C:/project','c:/project',0);
                 INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
                   VALUES ('chat-a','project-a',0,'Chat',1,1);",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO agent_sessions(
                   id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,
                   state,residency,created_at,updated_at,terminal_at
                 ) VALUES ('session-a','project-a','chat-a',0,'binding',?1,?2,
                           'closed','cold',1,1,1)",
                params!["a".repeat(64), CAPABILITIES],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_runtime_epochs(
                   project_id,chat_id,session_id,runtime_epoch,state,started_at,terminal_at
                 ) VALUES ('project-a','chat-a','session-a',1,'exited',1,1)",
                [],
            )
            .unwrap();
        let payload = "{\"kind\":\"text\",\"payload\":{\"text\":\"parent\"}}";
        let payload_digest = hex_digest(payload.as_bytes());
        for (turn_index, activity_count) in activity_counts.iter().copied().enumerate() {
            let turn_id = format!("turn-{turn_index}");
            let item_id = format!("item-{turn_index}");
            let created_at = 100_i64 - turn_index as i64;
            connection
                .execute(
                    "INSERT INTO turns(
                       id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                       state,delivery_state,created_at,updated_at,terminal_at
                     ) VALUES (?1,'project-a','chat-a','session-a',1,?2,?1,
                               'completed','correlated',?3,?3,?3)",
                    params![turn_id, turn_index as i64, created_at],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO items(
                       id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,
                       lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at
                     ) VALUES (?1,'project-a','chat-a','session-a',?2,1,0,'primary','text',
                               'completed',?3,?4,?5,?6,?6,?6)",
                    params![
                        item_id,
                        turn_id,
                        payload,
                        payload.len() as i64,
                        payload_digest,
                        created_at
                    ],
                )
                .unwrap();
            for activity_index in 0..activity_count {
                connection
                    .execute(
                        "INSERT INTO session_activities(
                           id,project_id,chat_id,session_id,turn_id,runtime_epoch,
                           external_activity_id,activity_kind,parent_item_id,role,lifecycle,
                           depth,observed_sequence,provenance_json,created_at,updated_at,terminal_at
                         ) VALUES (?1,'project-a','chat-a','session-a',?2,1,?1,'subagent',?3,
                                   'child','completed',1,?4,?5,?6,?6,?6)",
                        params![
                            format!("activity-{turn_index}-{activity_index}"),
                            turn_id,
                            item_id,
                            activity_index as i64 + 1,
                            PROVENANCE,
                            created_at
                        ],
                    )
                    .unwrap();
            }
        }
        drop(connection);
        database
    }
}
