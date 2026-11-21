use super::*;

const ALLOWED_TAB_KINDS: &[&str] = &[
    "chat", "file", "diff", "terminal", "search", "problems", "output", "settings", "browser",
];

pub(super) fn load_tabs(
    connection: &rusqlite::Connection,
    project_id: &str,
    roots: &HashMap<&str, &RootRecord>,
) -> AppResult<Vec<WorkbenchTab>> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM (
           SELECT 1 FROM workbench_tabs
           WHERE project_id = ?1 LIMIT ?2
         )",
            params![project_id, MAX_RESTORED_TABS as i64 + 1],
            |row| row.get(0),
        )
        .db()?;
    if count > MAX_RESTORED_TABS as i64 {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESTORED_TABS}-tab restore limit"
        )));
    }
    let bytes: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(
           length(CAST(id AS BLOB)) + length(CAST(kind AS BLOB))
           + length(CAST(title AS BLOB)) + length(CAST(metadata_json AS BLOB))
           + COALESCE(length(CAST(status AS BLOB)), 0)
         ), 0)
         FROM workbench_tabs WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .db()?;
    if bytes > MAX_RESTORED_TAB_BYTES {
        return Err(AppError::ResourceLimit(
            "persisted tabs exceed restore count or byte limits".into(),
        ));
    }
    let mut statement = connection
        .prepare(&format!(
        "SELECT w.id, w.kind, w.title, w.position, w.pinned, w.dirty, w.status, w.metadata_json,
                CASE
                  WHEN w.kind = 'chat' THEN COALESCE(c.updated_at, w.updated_at)
                  ELSE w.updated_at
                END,
                CASE WHEN w.kind = 'chat' THEN {} END,
                CASE WHEN w.kind = 'chat' THEN {} END
         FROM workbench_tabs w
         LEFT JOIN chat_tabs ct ON ct.project_id = w.project_id AND ct.tab_id = w.id
         LEFT JOIN chats c ON c.project_id = ct.project_id AND c.id = ct.chat_id
         WHERE w.project_id = ?1
         ORDER BY w.position LIMIT ?2",
        crate::project_registry::CHAT_FINISHED_AT,
        crate::chat_lifecycle::visibility::listed_chat()
    ))
        .db()?;
    let rows = statement
        .query_map(params![project_id, MAX_RESTORED_TABS as i64 + 1], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, i64>(5)? != 0,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<i64>>(9)?,
                row.get::<_, Option<bool>>(10)?,
            ))
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    if rows.len() > MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESTORED_TABS}-tab restore limit"
        )));
    }
    let mut ids = HashSet::new();
    let mut positions = HashSet::new();
    let tabs = rows
        .into_iter()
        .map(
            |(
                id,
                kind,
                title,
                position,
                pinned,
                dirty,
                status,
                metadata,
                updated_at,
                finished_at,
                chat_listed,
            )| {
                if !ids.insert(id.clone()) || !positions.insert(position) {
                    return Err(AppError::InvalidRequest(
                        "persisted layout contains duplicate tab identity or position".into(),
                    ));
                }
                if !ALLOWED_TAB_KINDS.contains(&kind.as_str()) {
                    return Err(AppError::InvalidRequest(format!(
                        "persisted tab has unsupported kind {kind}"
                    )));
                }
                let metadata: serde_json::Value = serde_json::from_str(&metadata)
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                let object = metadata.as_object().ok_or_else(|| {
                    AppError::InvalidRequest("persisted tab metadata is not an object".into())
                })?;
                if object.contains_key("resourcePath") {
                    return Err(AppError::InvalidRequest(
                        "persisted tab contains legacy resource metadata".into(),
                    ));
                }
                let resource = object
                    .get("resource")
                    .cloned()
                    .map(serde_json::from_value::<ResourceRef>)
                    .transpose()
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                let repository = object
                    .get("repository")
                    .cloned()
                    .map(serde_json::from_value::<RepositoryRef>)
                    .transpose()
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                let diff_ref = crate::workbench_diff_ref::parse_diff_ref_from_metadata(
                    project_id,
                    kind.as_str(),
                    resource.as_ref(),
                    repository.as_ref(),
                    object,
                )?;
                validate_tab_repository(&kind, resource.as_ref(), repository.as_ref())?;
                if let Some(repository) = &repository {
                    let root = roots.get(repository.root_id.as_str()).ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted tab repository root is foreign to its project".into(),
                        )
                    })?;
                    if !repository.relative_path.is_empty() {
                        validate_resource_metadata(
                            root,
                            &ResourceRef {
                                root_id: repository.root_id.clone(),
                                relative_path: repository.relative_path.clone(),
                            },
                        )?;
                    }
                }
                if kind == "file" || kind == "diff" {
                    let resource = resource.as_ref().ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted File and Diff tabs require a resource".into(),
                        )
                    })?;
                    let root = roots.get(resource.root_id.as_str()).ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted tab resource root is foreign to its project".into(),
                        )
                    })?;
                    validate_resource_metadata(root, resource)?;
                } else if resource.is_some() {
                    return Err(AppError::InvalidRequest(
                        "only persisted File and Diff tabs may carry a resource".into(),
                    ));
                }
                let restored_status = if kind == "browser" {
                    Some("closed".into())
                } else {
                    status
                };
                Ok(WorkbenchTab {
                    id,
                    kind,
                    title,
                    position,
                    pinned,
                    dirty,
                    status: restored_status,
                    updated_at: admit_tab_updated_at(updated_at),
                    finished_at: admit_tab_updated_at(finished_at),
                    chat_listed,
                    resource,
                    repository,
                    diff_ref,
                })
            },
        )
        .collect::<AppResult<Vec<_>>>()?;
    validate_tab_projection(&tabs)?;
    Ok(tabs)
}
