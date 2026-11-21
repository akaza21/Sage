use super::*;

/// Examine at most the bounded set of live runtimes. Each ledger lookup is
/// the latest title marker; the oldest eligible marker receives one wakeup.
pub(crate) fn pending_owner(
    connection: &Connection,
    owners: &[RuntimeOwner],
) -> Result<Option<RuntimeOwner>, StoreError> {
    if owners.is_empty() {
        return Ok(None);
    }
    let owners_json = serde_json::Value::Array(
        owners
            .iter()
            .map(|owner| {
                serde_json::json!({
                    "project": owner.project_id.as_str(), "chat": owner.chat_id.as_str(),
                    "session": owner.session_id.as_str(), "epoch": owner.runtime_epoch.get(),
                })
            })
            .collect(),
    )
    .to_string();
    let index: Option<i64> = connection.query_row(concat!(
        "SELECT live.key FROM json_each(?1) live
         JOIN chats c ON c.project_id=json_extract(live.value,'$.project') AND c.id=json_extract(live.value,'$.chat')
         JOIN chat_session_heads h ON h.project_id=c.project_id AND h.chat_id=c.id AND h.session_id=json_extract(live.value,'$.session')
         JOIN agent_sessions s ON s.id=h.session_id AND s.state IN ('ready','active')
         JOIN session_runtime_epochs e ON e.session_id=s.id AND e.runtime_epoch=json_extract(live.value,'$.epoch') AND e.state='running'
         JOIN turns t ON t.id=(SELECT t.id FROM turns t WHERE t.project_id=c.project_id AND t.chat_id=c.id
         AND t.state='completed' AND ", super::super::prompted_turn!("t.session_id", "t.id"),
         " ORDER BY t.created_at,t.ordinal,t.id LIMIT 1) AND t.session_id=s.id
         JOIN session_facts f ON f.rowid=(SELECT rowid FROM session_facts WHERE session_id=s.id AND turn_id=t.id AND kind='chatTitleAttempt' ORDER BY sequence DESC LIMIT 1)
         WHERE f.session_id=s.id AND json_extract(f.payload_json,'$.state')='pending'
         AND json_extract(f.payload_json,'$.tries')<3 AND c.title_source IN ('default','prompt') AND ",
        super::super::turn_undo::visible_turn!("t.session_id", "t.id"),
        " AND EXISTS(SELECT 1 FROM chat_tabs WHERE project_id=c.project_id AND chat_id=c.id)
         ORDER BY f.created_at,f.rowid LIMIT 1"
    ), [owners_json], |row| row.get(0)).optional()?;
    Ok(index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| owners.get(index).cloned()))
}
