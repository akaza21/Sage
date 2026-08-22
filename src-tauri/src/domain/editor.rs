use serde::{Deserialize, Serialize};

use super::ResourceRef;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    pub resource: ResourceRef,
    pub content: String,
    pub byte_len: u64,
    pub line_ending: String,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentVersion {
    pub fingerprint: String,
    pub byte_len: u64,
    pub modified_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorBufferVersion {
    pub fingerprint: String,
    pub byte_len: u64,
    pub revision: u64,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorConflict {
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDocument {
    pub resource: ResourceRef,
    pub disk: DocumentVersion,
    pub base: DocumentVersion,
    pub buffer: EditorBufferVersion,
    pub line_ending: String,
    pub dirty: bool,
    pub conflict: Option<EditorConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSnapshotRequest {
    pub resource: ResourceRef,
    pub base: DocumentVersion,
    pub line_ending: String,
    pub content: String,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSnapshotAck {
    pub revision: u64,
    pub persisted_at: u64,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSaveResult {
    pub outcome: String,
    pub document: EditorDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredEditorBuffer {
    pub resource: ResourceRef,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub resource: ResourceRef,
    pub hunks: Vec<DiffHunk>,
    pub binary: bool,
    pub truncated: bool,
}
