//! The Create project dialog's commands.
//!
//! The WebView never names a folder path. It asks for a native folder pick,
//! gets back an opaque token for the chosen folder, and creates the project
//! from those tokens. Only folders the person picked can become roots.

use std::{collections::VecDeque, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Window};
use uuid::Uuid;

use crate::{
    browser::BrowserState,
    domain::ProjectTransitionResult,
    error::{AppError, AppResult},
    project_commands::{execute_committed_transition_checked, pick_folder},
    project_registry::{MAX_ROOTS_PER_PROJECT, project_name},
    state::AppState,
};

/// Picks held for open Create project dialogs. The oldest is dropped first.
pub const MAX_PENDING_SOURCE_PICKS: usize = 32;

/// Folders the person picked in a Create project dialog, by token.
#[derive(Default)]
pub struct ProjectSourcePicks {
    picks: Mutex<VecDeque<(String, PathBuf)>>,
}

impl ProjectSourcePicks {
    /// Holds `path` and returns the token that names it.
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

    /// The folders `tokens` name, in order. Any unknown token fails the lot.
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
                database.create_named_project_if_active(
                    &name,
                    &roots,
                    expected_active_project_id.as_deref(),
                )
            })
        },
    )
    .await)
}

#[cfg(test)]
mod tests {
    use super::*;

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
