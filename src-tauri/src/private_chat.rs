//! Native ownership of project-less Chats. Paths are derived from the profile,
//! never accepted from the WebView. Native forks retain their source workspace
//! under the existing immutable same-project lineage rule. Empty private projects are cleanup receipts.
use crate::{
    chat_move::move_chat_in,
    project_registry::seed::{insert_root, seed_empty_project, seed_project},
    workbench_persistence::persist_layout_in,
};
use rusqlite::params;
use sage_core::{
    error::{AppError, AppResult},
    limits::LIMITS,
    time::now_millis,
};
use sage_db::{database::Database, error::SqliteResultExt};
use sage_domain::{
    chat::{MoveChatRequest, ProjectLayoutRequest},
    project::ProjectListItem,
};
use std::fs;
use uuid::Uuid;
mod cleanup;
pub(crate) const MAX_UNSCOPED_CHATS: i64 = 64;
const MAX_PRIVATE_PROJECTS: i64 = MAX_UNSCOPED_CHATS + 64;

fn storage_root(database: &Database) -> AppResult<std::path::PathBuf> {
    Ok(database.profile_directory()?.join("chat-workspaces"))
}

pub(crate) fn require_user_root(database: &Database, root: &std::path::Path) -> AppResult<()> {
    use crate::project_registry::canonical_comparison_key;
    let profile = match database.profile_directory() {
        Ok(profile) => profile,
        // Memory databases cannot own private workspace folders.
        Err(AppError::Unsupported(_)) => return Ok(()),
        Err(error) => return Err(error),
    };
    let base = profile.canonicalize()?.join("chat-workspaces");
    let base = base.canonicalize().unwrap_or(base);
    if std::path::Path::new(&canonical_comparison_key(root))
        .starts_with(canonical_comparison_key(&base))
    {
        return Err(AppError::InvalidRequest(
            "Sage's managed private Chat folders cannot be registered as project roots".into(),
        ));
    }
    Ok(())
}

fn allocate_folder(database: &Database) -> AppResult<(String, std::path::PathBuf)> {
    let project = Uuid::new_v4().to_string();
    let base = storage_root(database)?;
    fs::create_dir_all(&base)?;
    cleanup::admit_base(&base)?;
    let folder = base.canonicalize()?.join(&project);
    // Commit the ownership receipt before creating anything on disk. A crash
    // at any later point leaves an empty hidden project startup can reclaim.
    {
        let mut connection = database.write_connection()?;
        let transaction = connection.transaction().db()?;
        let count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE kind='privateChat'",
                [],
                |row| row.get(0),
            )
            .db()?;
        if count >= MAX_PRIVATE_PROJECTS {
            // Cleanup receipts are bounded independently of the live Chat quota.
            // Retire the oldest empty receipt at capacity; never delete its files
            // here. An unremovable folder cannot permanently block new Chats.
            cleanup::retire_oldest_empty(&transaction)?;
        }
        transaction.execute("INSERT INTO projects(id,name,kind,created_at,updated_at,last_opened_at) VALUES (?1,'Choose project','privateChat',?2,?2,?2)", params![project,now_millis()]).db()?;
        transaction.commit().db()?;
    }
    fs::create_dir(&folder)?;
    Ok((project, folder))
}

fn insert_in(
    transaction: &rusqlite::Transaction<'_>,
    project: &str,
    folder: &std::path::Path,
    chat: Option<&str>,
) -> AppResult<()> {
    let (chats, private): (i64, i64) = transaction
        .query_row(
            "SELECT (SELECT chat_count FROM acp_store_usage WHERE singleton=1),
                (SELECT COUNT(*) FROM projects WHERE kind='privateChat')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .db()?;
    if (chat.is_some() && chats >= LIMITS.durable.chats_global as i64)
        || private > MAX_PRIVATE_PROJECTS
    {
        return Err(AppError::ResourceLimit(
            "private Chat storage capacity reached".into(),
        ));
    }
    let now = now_millis();
    let root = Uuid::new_v4().to_string();
    insert_root(transaction, project, &root, folder, 0)?;
    match chat {
        Some(chat) => seed_project(transaction, project, chat, Some(&root), now),
        None => seed_empty_project(transaction, project, Some(&root), now),
    }
}

pub(crate) fn create(
    database: &Database,
    current: Option<&ProjectLayoutRequest>,
    chat: &str,
) -> AppResult<()> {
    if !Uuid::parse_str(chat).is_ok_and(|uuid| uuid.to_string() == chat) {
        return Err(AppError::InvalidRequest(
            "Chat identity must be a canonical UUID".into(),
        ));
    }
    let (project, folder) = allocate_folder(database)?;
    let result = (|| {
        let mut connection = database.write_connection()?;
        let transaction = connection.transaction().db()?;
        if let Some(current) = current {
            persist_layout_in(&transaction, current)?;
        }
        insert_in(&transaction, &project, &folder, Some(chat))?;
        transaction.commit().db()
    })();
    if result.is_err() {
        let _ = fs::remove_dir(&folder);
        cleanup_empty(database);
    }
    result
}

pub(crate) fn set_project(
    database: &Database,
    current: &ProjectLayoutRequest,
    chat: &str,
    target: Option<&str>,
) -> AppResult<()> {
    let allocation = target
        .is_none()
        .then(|| allocate_folder(database))
        .transpose()?;
    let result = (|| {
        let mut connection = database.write_connection()?;
        let transaction = connection.transaction().db()?;
        let sessions: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE chat_id=?1)",
                [chat],
                |row| row.get(0),
            )
            .db()?;
        if sessions {
            return Err(AppError::InvalidRequest(
                "A Chat with a session cannot change project".into(),
            ));
        }
        if let Some(target) = target {
            require_user_target(&transaction, target)?;
        }
        crate::side_chat_storage::refuse_persistence(&transaction, chat)?;
        // Keep allocation and re-parenting in the same native transaction:
        // failure changes neither ownership nor the active workbench.
        persist_layout_in(&transaction, current)?;
        let target = if let Some((project, folder)) = &allocation {
            insert_in(&transaction, project, folder, None)?;
            if !is_internal(&transaction, &current.project_id)? {
                admit_chat(&transaction, project, None)?;
            }
            project.as_str()
        } else {
            target.expect("target or allocation")
        };
        move_chat_in(
            &transaction,
            &MoveChatRequest {
                project_id: current.project_id.clone(),
                chat_id: chat.into(),
                target_project_id: target.into(),
                current_layout: current.clone(),
            },
        )?;
        transaction.commit().db()
    })();
    if result.is_err()
        && let Some((_, folder)) = allocation
    {
        let _ = fs::remove_dir(folder);
        cleanup_empty(database);
    }
    if result.is_ok() {
        cleanup_empty(database);
    }
    result
}

pub(crate) fn is_internal(connection: &rusqlite::Connection, project: &str) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT kind='privateChat' FROM projects WHERE id=?1",
            [project],
            |row| row.get(0),
        )
        .db()
}

pub(crate) fn require_user_target(
    connection: &rusqlite::Connection,
    target: &str,
) -> AppResult<()> {
    let user: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1 AND kind='user')",
            [target],
            |row| row.get(0),
        )
        .db()?;
    if !user {
        return Err(AppError::InvalidRequest(
            "Choose a registered user project".into(),
        ));
    }
    Ok(())
}

pub(crate) fn admit_chat(
    transaction: &rusqlite::Transaction<'_>,
    project: &str,
    parent: Option<&str>,
) -> AppResult<()> {
    let private: bool = transaction
        .query_row(
            "SELECT kind='privateChat' FROM projects WHERE id=?1",
            [project],
            |row| row.get(0),
        )
        .db()?;
    if !private || parent.is_some() {
        return Ok(());
    }
    let count: i64 = transaction.query_row("SELECT COUNT(*) FROM projects p JOIN chats c ON c.project_id=p.id WHERE p.kind='privateChat' AND c.id NOT LIKE 'side-%'", [], |row| row.get(0)).db()?;
    if count >= MAX_UNSCOPED_CHATS {
        return Err(AppError::ResourceLimit("Sage has reached its 64 chats without a project limit. Delete a chat or choose a project.".into()));
    }
    Ok(())
}

pub(crate) fn cleanup_empty(database: &Database) {
    if let Err(error) = cleanup::empty_projects(database) {
        tracing::warn!(%error, "private Chat cleanup will retry at startup");
    }
}

/// Separate from the public project registry; capped exactly like rail Chats.
/// Archived private Chats remain reachable here so they can be restored/deleted.
pub(crate) fn list(database: &Database) -> AppResult<Vec<ProjectListItem>> {
    let connection = database.connection();
    let mut select = connection.prepare(&format!("SELECT p.id,p.last_opened_at,c.id,c.title,
        COALESCE(w.pinned,0),c.ordinal,w.status,c.updated_at,
        EXISTS(SELECT 1 FROM chat_tabs WHERE chat_id=c.id), {}
        FROM projects p JOIN chats c ON c.project_id=p.id
        LEFT JOIN workbench_tabs w ON w.id=c.id
        WHERE p.kind='privateChat' AND c.id NOT LIKE 'side-%'
        ORDER BY (p.id IS (SELECT active_project_id FROM app_state WHERE singleton=1)) DESC,c.updated_at DESC,c.id
        LIMIT ?1", crate::project_registry::CHAT_FINISHED_AT)).db()?;
    let rows = select
        .query_map([MAX_UNSCOPED_CHATS], |row| {
            let chat_id: String = row.get(2)?;
            let title: String = row.get(3)?;
            let attached: bool = row.get(8)?;
            Ok(ProjectListItem {
                id: row.get(0)?,
                name: "".into(),
                root_count: 1,
                parent_path: None,
                last_opened_at: row.get(1)?,
                rail_chats: if attached {
                    vec![sage_domain::project::AgentRailChat {
                        id: chat_id.clone(),
                        title: title.clone(),
                        pinned: row.get(4)?,
                        position: row.get(5)?,
                        status: row.get(6)?,
                        updated_at: row.get(7)?,
                        finished_at: row.get(9)?,
                    }]
                } else {
                    vec![]
                },
                rail_chats_truncated: false,
                rail_archived: if attached {
                    vec![]
                } else {
                    vec![sage_domain::chat::ArchivedChatSummary { id: chat_id, title }]
                },
            })
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    let mut groups: Vec<ProjectListItem> = Vec::new();
    for row in rows {
        if let Some(group) = groups.iter_mut().find(|group| group.id == row.id) {
            group.rail_chats.extend(row.rail_chats);
            group.rail_archived.extend(row.rail_archived);
        } else {
            groups.push(row);
        }
    }
    Ok(groups)
}

#[cfg(test)]
mod cleanup_tests;
#[cfg(test)]
mod tests;
