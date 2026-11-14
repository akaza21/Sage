use std::sync::{Arc, atomic::AtomicBool};

use sage_core::error::{AppError, AppResult};

use crate::{
    domain::{BufferSnapshotRequest, ResourceRef, TrashResourceKind},
    editor,
    persistence::Database,
    workspace::{Workspace, file_rename::FileRenameMarker, resource_trash::ResourceTrashMarker},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentFileOperation {
    Created,
    Updated,
    Renamed,
    Deleted,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentFileIntent {
    pub(crate) operation: AgentFileOperation,
    pub(crate) resource: ResourceRef,
    pub(crate) source: Option<ResourceRef>,
    pub(crate) basename: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) project_id: String,
    pub(crate) project_generation: u64,
    pub(crate) expected_transition_epoch: u64,
}

pub(crate) fn apply_agent_file_intent(
    database: &Database,
    workspace: &Workspace,
    intent: AgentFileIntent,
) -> AppResult<()> {
    if workspace.project_id() != intent.project_id {
        return Err(AppError::InvalidRequest(
            "agent file intent Project does not own the workspace".into(),
        ));
    }
    match intent.operation {
        AgentFileOperation::Created => create_file(workspace, &intent),
        AgentFileOperation::Updated => update_file(database, workspace, &intent),
        AgentFileOperation::Renamed => rename_file(workspace, &intent),
        AgentFileOperation::Deleted => delete_file(workspace, &intent),
    }
}

fn create_file(workspace: &Workspace, intent: &AgentFileIntent) -> AppResult<()> {
    let basename = required_text(intent.basename.as_deref(), "basename")?;
    let parent = parent_resource(&intent.resource)?;
    let target = ResourceRef {
        root_id: parent.root_id.clone(),
        relative_path: join_resource_path(&parent.relative_path, basename),
    };
    if target != intent.resource {
        return Err(AppError::InvalidRequest(
            "agent file create target does not match its typed resource".into(),
        ));
    }
    let _ = workspace.create_empty_file_observed(
        &parent,
        basename,
        Arc::new(AtomicBool::new(false)),
    )?;
    Ok(())
}

fn join_resource_path(parent: &str, basename: &str) -> String {
    if parent.is_empty() {
        basename.into()
    } else {
        format!("{parent}/{basename}")
    }
}

fn update_file(
    database: &Database,
    workspace: &Workspace,
    intent: &AgentFileIntent,
) -> AppResult<()> {
    let document = editor::open_document(database, workspace, &intent.resource)?;
    let revision = document.buffer.revision.checked_add(1).ok_or_else(|| {
        AppError::InvalidRequest("agent file update revision is exhausted".into())
    })?;
    editor::snapshot_buffer(
        database,
        workspace,
        BufferSnapshotRequest {
            resource: intent.resource.clone(),
            base: document.base,
            line_ending: document.line_ending,
            content: required_content(intent.content.as_deref())?.into(),
            revision,
        },
    )?;
    let _ = editor::save_document(database, workspace, &intent.resource)?;
    Ok(())
}

fn rename_file(workspace: &Workspace, intent: &AgentFileIntent) -> AppResult<()> {
    let source = required_resource(intent.source.as_ref(), "source")?;
    let basename = required_text(intent.basename.as_deref(), "basename")?;
    let target = workspace.rename_file_target(source, basename)?;
    if target != intent.resource {
        return Err(AppError::InvalidRequest(
            "agent file rename target does not match its typed resource".into(),
        ));
    }
    let _ =
        workspace.rename_file_observed(source, basename, Arc::new(FileRenameMarker::default()))?;
    Ok(())
}

fn delete_file(workspace: &Workspace, intent: &AgentFileIntent) -> AppResult<()> {
    let kind = TrashResourceKind::File;
    let identity = workspace.resource_trash_identity(&intent.resource, kind)?;
    let _ = workspace.trash_resource_observed(
        &intent.resource,
        kind,
        identity,
        Arc::new(ResourceTrashMarker::default()),
    )?;
    Ok(())
}

fn parent_resource(resource: &ResourceRef) -> AppResult<ResourceRef> {
    let (parent, _) = resource.relative_path.rsplit_once('/').unwrap_or(("", ""));
    if resource.relative_path.is_empty() {
        return Err(AppError::InvalidRequest(
            "agent file creation resource must name a file".into(),
        ));
    }
    Ok(ResourceRef {
        root_id: resource.root_id.clone(),
        relative_path: parent.into(),
    })
}

fn required_text<'a>(value: Option<&'a str>, label: &str) -> AppResult<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::InvalidRequest(format!("agent file intent {label} is required")))
}

fn required_content(value: Option<&str>) -> AppResult<&str> {
    value.ok_or_else(|| AppError::InvalidRequest("agent file intent content is required".into()))
}

fn required_resource<'a>(
    value: Option<&'a ResourceRef>,
    label: &str,
) -> AppResult<&'a ResourceRef> {
    value.ok_or_else(|| AppError::InvalidRequest(format!("agent file intent {label} is required")))
}

#[cfg(test)]
mod tests;
