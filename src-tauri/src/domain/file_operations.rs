use serde::{Deserialize, Serialize};

use super::{DirectoryListing, ProjectRepositoryStatus, ResourceRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitFileMutationAction {
    Stage,
    Unstage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitFileMutationRequest {
    pub project_id: String,
    pub resource: ResourceRef,
    pub action: GitFileMutationAction,
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
