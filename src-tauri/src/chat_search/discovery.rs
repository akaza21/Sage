use super::*;

pub(super) fn load_chats(
    connection: &rusqlite::Connection,
    project_id: &str,
    query: &str,
    limits: SearchLimits,
    draft_chat_ids: &[String],
) -> AppResult<Vec<ChatRow>> {
    let row_limit = if query.is_empty() {
        limits.max_results
    } else {
        limits.max_chat_rows
    };
    let limit = i64::try_from(row_limit.saturating_add(1)).unwrap_or(i64::MAX);
    let mut statement = connection.prepare(&format!(
        "SELECT c.id,c.title,EXISTS(
           SELECT 1 FROM chat_tabs a WHERE a.project_id=c.project_id AND a.chat_id=c.id
         ),c.updated_at
         FROM chats c WHERE c.project_id=?1 AND ({} OR c.id IN (SELECT value FROM json_each(?3))) AND c.id NOT IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1)
         ORDER BY c.updated_at DESC,c.ordinal DESC,c.id ASC LIMIT ?2",
        crate::chat_lifecycle::visibility::listed_chat()
    )).db()?;
    let rows = statement
        .query_map(
            params![
                project_id,
                limit,
                serde_json::to_string(draft_chat_ids)
                    .map_err(|e| AppError::InvalidRequest(e.to_string()))?
            ],
            |row| {
                let title: String = row.get(1)?;
                Ok(ChatRow {
                    id: row.get(0)?,
                    matched_title: title.contains(query),
                    title,
                    attached: row.get(2)?,
                    snippet: None,
                    updated_at: row.get(3)?,
                })
            },
        )
        .db()?;
    let chats = rows.collect::<Result<Vec<_>, _>>().db()?;
    if !query.is_empty() && chats.len() > limits.max_chat_rows {
        return Err(AppError::ResourceLimit(
            "Chat search Chat-row ceiling was exceeded".into(),
        ));
    }
    Ok(chats)
}
