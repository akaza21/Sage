use serde::{Deserialize, Serialize};

use super::{SafeResourceLocation, SafeText};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileChangeOperation {
    #[default]
    Updated,
    Created,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileChangeFacts {
    pub(crate) resource: SafeResourceLocation,
    pub(crate) summary: SafeText<65536>,
    pub(crate) additions: Option<u64>,
    pub(crate) deletions: Option<u64>,
    pub(crate) operation: FileChangeOperation,
}
