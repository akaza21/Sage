//! The workspace Replace commands. A preview reads each File and says what
//! a Replace would change and the fingerprint it saw; an apply writes each
//! File only if it still has that fingerprint.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::{
    replace::{
        MAX_PREVIEW_LINES, MAX_PREVIEW_LINES_PER_FILE, MatchPosition, ReplaceLinePreview,
        ReplaceSpec, Replacer,
    },
    types::{MAX_PROJECT_ID_BYTES, MAX_SEARCH_RESULTS, MAX_SEARCH_TOTAL_BYTES},
};
use crate::{
    domain::ResourceRef,
    editor::{DiskReplace, read_for_replace, replace_on_disk},
    editor_content::MAX_EDITABLE_BYTES,
    editor_coordination::EditorOperationKind,
    error::{AppError, AppResult},
    persistence::Database,
    state::AppState,
};

/// The Files one Replace may name: every File a Search's pages can show.
pub const MAX_REPLACE_FILES: usize = MAX_SEARCH_RESULTS * 4;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceReplacePreviewRequest {
    pub project_id: String,
    pub expected_transition_epoch: u64,
    pub spec: ReplaceSpec,
    pub resources: Vec<ResourceRef>,
    /// Preview only the match Search reported here.
    #[serde(default)]
    pub only: Option<MatchPosition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceTarget {
    pub resource: ResourceRef,
    /// The fingerprint the preview read.
    pub fingerprint: String,
    /// Replace only the match Search reported here.
    #[serde(default)]
    pub only: Option<MatchPosition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceReplaceApplyRequest {
    pub project_id: String,
    pub expected_transition_epoch: u64,
    pub spec: ReplaceSpec,
    pub files: Vec<ReplaceTarget>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextReplaceRequest {
    pub spec: ReplaceSpec,
    pub text: String,
    #[serde(default)]
    pub only: Option<MatchPosition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceFilePreview {
    pub resource: ResourceRef,
    pub fingerprint: String,
    pub count: u64,
    pub lines: Vec<ReplaceLinePreview>,
    pub lines_truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplaceProblemKind {
    /// Changed since the preview; nothing was written.
    Stale,
    /// Not replaced: protected, too large, unreadable.
    Refused,
    NoMatches,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceFileProblem {
    pub resource: ResourceRef,
    pub kind: ReplaceProblemKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReplacePreviewResponse {
    pub files: Vec<ReplaceFilePreview>,
    pub problems: Vec<ReplaceFileProblem>,
    /// The byte budget ran out; the Files past it were not read.
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplacedFile {
    pub resource: ResourceRef,
    pub count: u64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReplaceApplyResponse {
    pub replaced: Vec<ReplacedFile>,
    pub problems: Vec<ReplaceFileProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReplaceResponse {
    pub content: String,
    pub count: u64,
    pub lines: Vec<ReplaceLinePreview>,
    pub lines_truncated: bool,
}

#[tauri::command]
pub async fn preview_workspace_replace(
    state: State<'_, AppState>,
    request: WorkspaceReplacePreviewRequest,
) -> Result<WorkspaceReplacePreviewResponse, String> {
    validate_files(&request.project_id, request.resources.len())?;
    let replacer = Replacer::new(&request.spec).map_err(|error| error.to_string())?;
    run_in_project(
        &state,
        &request.project_id,
        request.expected_transition_epoch,
        move |database| {
            Ok(preview(
                database,
                &replacer,
                &request.resources,
                request.only,
            ))
        },
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn apply_workspace_replace(
    state: State<'_, AppState>,
    request: WorkspaceReplaceApplyRequest,
) -> Result<WorkspaceReplaceApplyResponse, String> {
    validate_files(&request.project_id, request.files.len())?;
    let replacer = Replacer::new(&request.spec).map_err(|error| error.to_string())?;
    run_in_project(
        &state,
        &request.project_id,
        request.expected_transition_epoch,
        move |database| Ok(apply(database, &replacer, &request.files)),
    )
    .await
    .map_err(|error| error.to_string())
}

/// What a Replace makes of text the editor holds, such as unsaved edits.
#[tauri::command]
pub async fn replace_workspace_text(
    request: TextReplaceRequest,
) -> Result<TextReplaceResponse, String> {
    replace_text(&request).map_err(|error| error.to_string())
}

pub fn replace_text(request: &TextReplaceRequest) -> AppResult<TextReplaceResponse> {
    if request.text.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(AppError::ResourceLimit(
            "the text exceeds the editor's size limit".into(),
        ));
    }
    let done = Replacer::new(&request.spec)?.replace(
        &request.text,
        request.only,
        MAX_PREVIEW_LINES_PER_FILE,
    );
    if done.content.len() as u64 > MAX_EDITABLE_BYTES {
        return Err(AppError::ResourceLimit(
            "the replaced text would exceed the editor's size limit".into(),
        ));
    }
    Ok(TextReplaceResponse {
        content: done.content,
        count: done.count,
        lines: done.lines,
        lines_truncated: done.lines_truncated,
    })
}

fn validate_files(project_id: &str, count: usize) -> Result<(), String> {
    if project_id.is_empty() || project_id.len() > MAX_PROJECT_ID_BYTES {
        return Err("invalid request: workspace replace project ID is invalid".into());
    }
    if count == 0 || count > MAX_REPLACE_FILES {
        return Err(format!(
            "resource limit exceeded: a workspace replace names 1 to {MAX_REPLACE_FILES} files"
        ));
    }
    Ok(())
}

/// Runs `operation` in turn with the editor's own operations, while the
/// project the request was made in stays active.
async fn run_in_project<T: Send + 'static>(
    state: &AppState,
    project_id: &str,
    expected_epoch: u64,
    operation: impl FnOnce(&Database) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let epoch = state.project_transitions.lock().await;
    if *epoch != expected_epoch
        || state.database.active_project_id()?.as_deref() != Some(project_id)
    {
        return Err(AppError::InvalidRequest(
            "project changed before workspace replace could run".into(),
        ));
    }
    let database = state.database.clone();
    let operations = state.editor_operations.clone();
    let ticket = operations.issue_ticket()?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        operations.execute(ticket, EditorOperationKind::Terminal, || {
            operation(&database)
        })
    })
    .await
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    drop(epoch);
    result
}

pub(crate) fn preview(
    database: &Database,
    replacer: &Replacer,
    resources: &[ResourceRef],
    only: Option<MatchPosition>,
) -> WorkspaceReplacePreviewResponse {
    let mut files = Vec::new();
    let mut problems = Vec::new();
    let mut bytes = 0_u64;
    let mut lines_left = MAX_PREVIEW_LINES;
    let mut truncated = false;
    for resource in resources {
        let read = database
            .resolve_active_resource(resource)
            .and_then(|workspace| read_for_replace(database, &workspace, resource));
        let source = match read {
            Ok(source) => source,
            Err(error) => {
                problems.push(problem(resource, ReplaceProblemKind::Refused, &error));
                continue;
            }
        };
        bytes += source.content.len() as u64;
        if bytes > MAX_SEARCH_TOTAL_BYTES {
            truncated = true;
            break;
        }
        let done = replacer.replace(
            &source.content,
            only,
            lines_left.min(MAX_PREVIEW_LINES_PER_FILE),
        );
        if done.count == 0 {
            problems.push(ReplaceFileProblem {
                resource: source.resource,
                kind: ReplaceProblemKind::NoMatches,
                message: "nothing in it matches now".into(),
            });
            continue;
        }
        lines_left -= done.lines.len();
        files.push(ReplaceFilePreview {
            resource: source.resource,
            fingerprint: source.fingerprint,
            count: done.count,
            lines_truncated: done.lines_truncated,
            lines: done.lines,
        });
    }
    WorkspaceReplacePreviewResponse {
        files,
        problems,
        truncated,
    }
}

pub(crate) fn apply(
    database: &Database,
    replacer: &Replacer,
    targets: &[ReplaceTarget],
) -> WorkspaceReplaceApplyResponse {
    let mut replaced = Vec::new();
    let mut problems = Vec::new();
    for target in targets {
        let written = database
            .resolve_active_resource(&target.resource)
            .and_then(|workspace| {
                replace_on_disk(
                    database,
                    &workspace,
                    &target.resource,
                    &target.fingerprint,
                    |content| {
                        let done = replacer.replace(content, target.only, 0);
                        (done.content, done.count)
                    },
                )
            });
        match written {
            Ok(DiskReplace::Written { fingerprint, count }) => replaced.push(ReplacedFile {
                resource: target.resource.clone(),
                count,
                fingerprint,
            }),
            Ok(DiskReplace::Stale) => problems.push(ReplaceFileProblem {
                resource: target.resource.clone(),
                kind: ReplaceProblemKind::Stale,
                message: "it changed since the preview; search again to replace in it".into(),
            }),
            Ok(DiskReplace::NoMatches) => problems.push(ReplaceFileProblem {
                resource: target.resource.clone(),
                kind: ReplaceProblemKind::NoMatches,
                message: "nothing in it matches now".into(),
            }),
            Err(error) => problems.push(problem(
                &target.resource,
                ReplaceProblemKind::Refused,
                &error,
            )),
        }
    }
    WorkspaceReplaceApplyResponse { replaced, problems }
}

fn problem(
    resource: &ResourceRef,
    kind: ReplaceProblemKind,
    error: &AppError,
) -> ReplaceFileProblem {
    let message = error.to_string();
    ReplaceFileProblem {
        resource: resource.clone(),
        kind,
        message: message
            .strip_prefix("invalid request: ")
            .or_else(|| message.strip_prefix("resource limit exceeded: "))
            .unwrap_or(&message)
            .into(),
    }
}

#[cfg(test)]
#[path = "replace_command_tests.rs"]
mod tests;
