//! The WebView never names a folder path. It asks for a native folder pick,
//! gets back an opaque token for the chosen folder, and creates the project
//! from those tokens. Only folders the person picked can become roots.

use std::{collections::VecDeque, path::PathBuf, sync::Mutex};

use sage_core::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Window};
use uuid::Uuid;

use crate::project_registry::create::ProjectCreateStore;
use crate::{
    browser::BrowserState,
    project_commands::{execute_committed_transition_checked, pick_folder},
    project_registry::{MAX_ROOTS_PER_PROJECT, project_name},
    state::AppState,
};
use sage_domain::project::ProjectTransitionResult;

/// The oldest pick is dropped first.
pub const MAX_PENDING_SOURCE_PICKS: usize = 32;

#[derive(Default)]
pub struct ProjectSourcePicks {
    picks: Mutex<VecDeque<(String, PathBuf)>>,
}

impl ProjectSourcePicks {
    pub fn admit(&self, path: PathBuf) -> String {
        let token = Uuid::new_v4().to_string();
        let mut picks = self
            .picks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if picks.len() >= MAX_PENDING_SOURCE_PICKS {
            picks.pop_front();
        }
        picks.push_back((token.clone(), path));
        token
    }

    /// Any unknown token fails the lot.
    pub fn resolve(&self, tokens: &[String]) -> AppResult<Vec<PathBuf>> {
        let picks = self
            .picks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tokens
            .iter()
            .map(|token| {
                picks
                    .iter()
                    .find(|(held, _)| held == token)
                    .map(|(_, path)| path.clone())
                    .ok_or_else(|| {
                        AppError::InvalidRequest(
                            "a source folder is no longer available; add it again".into(),
                        )
                    })
            })
            .collect()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceFolder {
    pub token: String,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectRequest {
    pub name: String,
    pub source_tokens: Vec<String>,
    pub source_chat_id: Option<String>,
}

#[tauri::command]
pub async fn pick_project_source_folder(
    app: AppHandle,
    window: Window,
    picks: State<'_, ProjectSourcePicks>,
) -> Result<Option<ProjectSourceFolder>, String> {
    let Some(selected) = pick_folder(&app, &window, "Add a folder").await? else {
        return Ok(None);
    };
    let canonical = selected.canonicalize().map_err(|error| error.to_string())?;
    if !canonical.is_dir() {
        return Err("the selected folder is not a directory".into());
    }
    let path = canonical
        .to_str()
        .ok_or("the selected folder's path is not valid UTF-8")?
        .to_owned();
    let name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&path)
        .to_owned();
    Ok(Some(ProjectSourceFolder {
        token: picks.admit(canonical),
        name,
        path,
    }))
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    browser: State<'_, BrowserState>,
    picks: State<'_, ProjectSourcePicks>,
    request: CreateProjectRequest,
    expected_active_project_id: Option<String>,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    if request.source_tokens.len() > MAX_ROOTS_PER_PROJECT as usize {
        return Err(format!(
            "a project holds at most {MAX_ROOTS_PER_PROJECT} source folders"
        ));
    }
    let name = project_name(&request.name)
        .map_err(|error| error.to_string())?
        .to_owned();
    let roots = picks
        .resolve(&request.source_tokens)
        .map_err(|error| error.to_string())?;
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations
        .issue_ticket()
        .map_err(|error| error.to_string())?;
    let preflight_roots = roots.clone();
    Ok(execute_committed_transition_checked(
        &state,
        expected_transition_epoch,
        || {
            browser
                .validate_project_roots(&preflight_roots)
                .map_err(AppError::InvalidRequest)
        },
        move || {
            operations.execute_project_transition(ticket, || {
                create_for_source(
                    &database,
                    &name,
                    &roots,
                    expected_active_project_id.as_deref(),
                    request.source_chat_id.as_deref(),
                )
            })
        },
    )
    .await)
}

fn create_for_source(
    database: &sage_db::database::Database,
    name: &str,
    roots: &[PathBuf],
    expected_active_project_id: Option<&str>,
    source_chat_id: Option<&str>,
) -> AppResult<sage_domain::project::ProjectSummary> {
    if let Some(chat) = source_chat_id {
        let source = expected_active_project_id
            .ok_or_else(|| AppError::InvalidRequest("Chat project is unavailable".into()))?;
        crate::private_chat::require_movable_source(database, source, chat)?;
    }
    database.create_named_project_if_active(name, roots, expected_active_project_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_checks_the_source_chat_before_creation_or_switching() {
        use crate::session_events::SessionEventStore;
        use crate::workbench_persistence::store::WorkbenchStore;
        let root = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("project-chat-preflight-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let chat = Uuid::new_v4().to_string();
        crate::private_chat::create(&database, None, &chat).unwrap();
        let before = database.bootstrap_snapshot().unwrap();
        let source = before.project.unwrap().id;
        let folder = root.join("chat-workspaces").join(&source);
        std::fs::write(folder.join("notes.txt"), "Keep my saved work").unwrap();
        let result = create_for_source(&database, "Destination", &[], Some(&source), Some(&chat));
        assert!(
            result
                .unwrap_err()
                .public_message()
                .contains("This chat's folder has files.")
        );
        let rejected = database.bootstrap_snapshot().unwrap();
        assert!(rejected.projects.is_empty());
        assert_eq!(rejected.project.unwrap().id, source);
        assert_eq!(rejected.active_tab_id.as_deref(), Some(chat.as_str()));
        assert_eq!(
            std::fs::read_to_string(folder.join("notes.txt")).unwrap(),
            "Keep my saved work"
        );
        std::fs::remove_file(folder.join("notes.txt")).unwrap();
        database
            .append_simulator_turn(&chat, "hello", "world")
            .unwrap();
        let result = create_for_source(&database, "Destination", &[], Some(&source), Some(&chat));
        assert!(
            result
                .unwrap_err()
                .public_message()
                .contains("session cannot change project")
        );
        assert!(database.bootstrap_snapshot().unwrap().projects.is_empty());
        // An unsent Chat with an empty folder can create a real named project.
        let unsent = Uuid::new_v4().to_string();
        crate::private_chat::create(&database, None, &unsent).unwrap();
        let source = database.bootstrap_snapshot().unwrap().project.unwrap().id;
        let created =
            create_for_source(&database, "Destination", &[], Some(&source), Some(&unsent)).unwrap();
        let after = database.bootstrap_snapshot().unwrap();
        assert_eq!(after.project.unwrap().id, created.id);
        assert_eq!(after.projects.len(), 1);
        assert_eq!(created.name, "Destination");
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tokens_resolve_in_order_and_unknown_tokens_fail() {
        let picks = ProjectSourcePicks::default();
        let web = picks.admit(PathBuf::from("web"));
        let api = picks.admit(PathBuf::from("api"));
        assert_eq!(
            picks.resolve(&[api.clone(), web.clone()]).unwrap(),
            [PathBuf::from("api"), PathBuf::from("web")]
        );
        assert!(picks.resolve(&[web, "forged".into()]).is_err());
    }

    #[test]
    fn the_oldest_pick_is_dropped_past_the_limit() {
        let picks = ProjectSourcePicks::default();
        let first = picks.admit(PathBuf::from("first"));
        let rest: Vec<_> = (0..MAX_PENDING_SOURCE_PICKS)
            .map(|index| picks.admit(PathBuf::from(index.to_string())))
            .collect();
        assert!(picks.resolve(&[first]).is_err());
        assert_eq!(
            picks.resolve(&rest).unwrap().len(),
            MAX_PENDING_SOURCE_PICKS
        );
    }
}
