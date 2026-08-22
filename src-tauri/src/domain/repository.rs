use serde::{Deserialize, Serialize};

use super::ResourceRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub resource: ResourceRef,
    pub original_resource: Option<ResourceRef>,
    pub index_status: String,
    pub worktree_status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootRepositoryStatus {
    pub root_id: String,
    pub state: String,
    pub branch: Option<String>,
    pub changes: Vec<FileChange>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    pub message: Option<String>,
}

impl RootRepositoryStatus {
    pub fn unavailable(root_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            root_id: root_id.into(),
            state: "unavailable".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms: 0,
            message: Some(message.into()),
        }
    }

    pub fn not_repository(root_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            root_id: root_id.into(),
            state: "notRepository".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms,
            message: None,
        }
    }

    pub fn repository_subdirectory(root_id: impl Into<String>, elapsed_ms: u64) -> Self {
        Self {
            root_id: root_id.into(),
            state: "notRepository".into(),
            branch: None,
            changes: Vec::new(),
            truncated: false,
            elapsed_ms,
            message: Some(
                "This folder is inside a parent Git repository; select the repository top level for Source Control."
                    .into(),
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRepositoryStatus {
    pub roots: Vec<RootRepositoryStatus>,
    pub truncated: bool,
    pub elapsed_ms: u64,
}
