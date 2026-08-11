use serde::{Deserialize, Serialize};

use super::{
    BootstrapSnapshot, DirectoryListing, ProjectLayoutRequest, ProjectRepositoryStatus, ResourceRef,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitFileMutationAction {
    Stage,
    Unstage,
    Discard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitFileMutationRequest {
    pub project_id: String,
    pub resource: ResourceRef,
    pub action: GitFileMutationAction,
    pub discard_confirmed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitFileMutationErrorKind {
    EffectUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileMutationError {
    pub kind: GitFileMutationErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileMutationResult {
    pub project_id: String,
    pub transition_epoch: u64,
    pub repositories: ProjectRepositoryStatus,
    pub error: Option<GitFileMutationError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitProcessState {
    TerminatedUnreaped,
    PotentiallyRunning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GitFileMutationCommandError {
    Failed {
        message: String,
    },
    Containment {
        process_state: GitProcessState,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryCommitRequest {
    pub project_id: String,
    pub root_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryCommitResult {
    pub project_id: String,
    pub root_id: String,
    pub transition_epoch: u64,
    pub repositories: ProjectRepositoryStatus,
    pub error: Option<GitFileMutationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectFileRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub parent: ResourceRef,
    pub basename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileCreationOutcome {
    Committed,
    EffectUnknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectFileResult {
    pub outcome: FileCreationOutcome,
    pub resource_confirmed: bool,
    pub project_id: String,
    pub project_generation: u64,
    pub transition_epoch: u64,
    pub resource: ResourceRef,
    pub parent_listing: DirectoryListing,
    pub warning: Option<String>,
}

pub type CreateProjectFolderRequest = CreateProjectFileRequest;
pub type CreateProjectFolderResult = CreateProjectFileResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenameProjectFileRequest {
    pub project_id: String,
    pub project_generation: u64,
    pub expected_transition_epoch: u64,
    pub source: ResourceRef,
    pub basename: String,
    pub current_layout: ProjectLayoutRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileRenameOutcome {
    Committed,
    EffectUnknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameProjectFileResult {
    pub outcome: FileRenameOutcome,
    pub resource_confirmed: bool,
    pub project_id: String,
    pub project_generation: u64,
    pub transition_epoch: u64,
    pub source: ResourceRef,
    pub target: ResourceRef,
    pub parent_listing: Option<DirectoryListing>,
    pub snapshot: Option<BootstrapSnapshot>,
    pub warning: Option<String>,
}
