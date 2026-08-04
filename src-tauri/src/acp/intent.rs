use serde::{Deserialize, Deserializer, Serialize, de};

use super::contract::{
    BoundedVec, ContractError, ItemKind, RequiredText, SafeText, SageRootId, SchemaVersionV1,
};

pub type BindingId = RequiredText<512>;
pub type ModelId = RequiredText<512>;
pub type ExternalId = RequiredText<512>;
pub type PageCursor = RequiredText<512>;
pub type Prompt = RequiredText<100000>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SafeResourceRef {
    pub root_id: SageRootId,
    pub relative_path: SafeText<16384>,
}

impl SafeResourceRef {
    pub fn new(root_id: SageRootId, path: impl Into<String>) -> Result<Self, ContractError> {
        let path = path.into();
        if path.is_empty()
            || path.starts_with('/')
            || path.contains('\\')
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || path.as_bytes().get(1) == Some(&b':')
        {
            return Err(ContractError::ForbiddenContent {
                field: "relativePath",
            });
        }
        Ok(Self {
            root_id,
            relative_path: SafeText::new("relativePath", path)?,
        })
    }
}

impl<'de> Deserialize<'de> for SafeResourceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WireRef {
            root_id: SageRootId,
            relative_path: String,
        }
        let value = WireRef::deserialize(deserializer)?;
        Self::new(value.root_id, value.relative_path).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UnavailableResourceReason {
    AmbiguousRoot,
    Foreign,
    Missing,
    Noncanonical,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SafeResourceLocation {
    Resource {
        resource: SafeResourceRef,
    },
    Unavailable {
        reason: UnavailableResourceReason,
        label: SafeText<512>,
    },
}

impl SafeResourceLocation {
    pub(crate) fn unavailable(reason: UnavailableResourceReason) -> Self {
        let label = match reason {
            UnavailableResourceReason::AmbiguousRoot => "Path belongs to multiple project roots",
            UnavailableResourceReason::Foreign => "Path is outside the active project roots",
            UnavailableResourceReason::Missing => "Path does not exist",
            UnavailableResourceReason::Noncanonical => "Path is not canonical",
            UnavailableResourceReason::Unsupported => "Path cannot be represented safely",
        };
        Self::Unavailable {
            reason,
            label: SafeText::new("resourceUnavailable", label)
                .expect("fixed unavailable-resource label is bounded"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCategory {
    File,
    Diff,
    Command,
    Search,
    Web,
    Mcp,
    Subagent,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum NormalizedPayload {
    Text {
        text: SafeText<262144>,
    },
    ThoughtSummary {
        summary: SafeText<262144>,
    },
    Plan {
        summary: SafeText<262144>,
    },
    ToolCall {
        category: ToolCategory,
        label: SafeText<512>,
        input_summary: Option<SafeText<65536>>,
    },
    ToolUpdateResult {
        category: ToolCategory,
        label: SafeText<512>,
        output_summary: Option<SafeText<65536>>,
    },
    FileChangeDiff {
        resource: SafeResourceLocation,
        summary: SafeText<65536>,
        additions: Option<u64>,
        deletions: Option<u64>,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    PermissionRequest {
        category: ToolCategory,
        summary: SafeText<65536>,
    },
    Error {
        code: SafeText<128>,
        message: SafeText<8192>,
        retryable: bool,
    },
    Unsupported {
        external_kind: SafeText<128>,
        summary: SafeText<16384>,
    },
    Collaboration {
        label: SafeText<512>,
        message: Option<SafeText<16384>>,
        receiver_labels: BoundedVec<SafeText<512>, 32>,
    },
}

impl NormalizedPayload {
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Text { .. } => ItemKind::Text,
            Self::ThoughtSummary { .. } => ItemKind::ThoughtSummary,
            Self::Plan { .. } => ItemKind::Plan,
            Self::ToolCall { .. } => ItemKind::ToolCall,
            Self::ToolUpdateResult { .. } => ItemKind::ToolUpdateResult,
            Self::FileChangeDiff { .. } => ItemKind::FileChangeDiff,
            Self::Usage { .. } => ItemKind::Usage,
            Self::PermissionRequest { .. } => ItemKind::PermissionRequest,
            Self::Error { .. } => ItemKind::Error,
            Self::Unsupported { .. } => ItemKind::Unsupported,
            Self::Collaboration { .. } => ItemKind::Collaboration,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshotV1 {
    pub schema_version: SchemaVersionV1,
    pub protocol_version: RequiredText<64>,
    pub new_session: bool,
    pub resume_session: bool,
    pub load_session: bool,
    pub close_session: bool,
    pub prompt: bool,
    pub cancel: bool,
    pub native_session_fork: bool,
    pub active_turn_steer: bool,
    pub child_conversation_projection: bool,
    pub child_transcript_read: bool,
    pub collaboration_event_projection: bool,
    pub multi_root_extension_schema_id: Option<RequiredText<128>>,
    pub model_count: u16,
    pub config_count: u16,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CapabilitySnapshotWire {
    schema_version: SchemaVersionV1,
    protocol_version: RequiredText<64>,
    new_session: bool,
    resume_session: bool,
    load_session: bool,
    close_session: bool,
    prompt: bool,
    cancel: bool,
    native_session_fork: bool,
    active_turn_steer: bool,
    child_conversation_projection: bool,
    child_transcript_read: bool,
    collaboration_event_projection: bool,
    multi_root_extension_schema_id: Option<RequiredText<128>>,
    model_count: u16,
    config_count: u16,
}

impl<'de> Deserialize<'de> for CapabilitySnapshotV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CapabilitySnapshotWire::deserialize(deserializer)?;
        if wire.model_count > 128 || wire.config_count > 128 {
            return Err(de::Error::custom(
                "invalid capability snapshot version or count",
            ));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            protocol_version: wire.protocol_version,
            new_session: wire.new_session,
            resume_session: wire.resume_session,
            load_session: wire.load_session,
            close_session: wire.close_session,
            prompt: wire.prompt,
            cancel: wire.cancel,
            native_session_fork: wire.native_session_fork,
            active_turn_steer: wire.active_turn_steer,
            child_conversation_projection: wire.child_conversation_projection,
            child_transcript_read: wire.child_transcript_read,
            collaboration_event_projection: wire.collaboration_event_projection,
            multi_root_extension_schema_id: wire.multi_root_extension_schema_id,
            model_count: wire.model_count,
            config_count: wire.config_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivitySourceKind {
    Tool,
    Extension,
    Collaboration,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityProvenanceV1 {
    pub schema_version: SchemaVersionV1,
    pub source_kind: ActivitySourceKind,
    pub external_kind: RequiredText<128>,
    pub extension_schema_id: Option<RequiredText<128>>,
    pub safe_label: Option<RequiredText<512>>,
}

impl ActivityProvenanceV1 {
    pub(crate) fn validate_for_persistence(&self) -> Result<(), ContractError> {
        let fields = [
            self.external_kind.as_str(),
            self.extension_schema_id
                .as_ref()
                .map_or("", RequiredText::as_str),
            self.safe_label.as_ref().map_or("", RequiredText::as_str),
        ];
        if fields.into_iter().any(contains_absolute_path_authority) {
            return Err(ContractError::ForbiddenContent {
                field: "activityProvenance",
            });
        }
        Ok(())
    }
}

fn contains_absolute_path_authority(value: &str) -> bool {
    value.starts_with('/')
        || value.starts_with("\\\\")
        || value.as_bytes().get(1) == Some(&b':')
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
            && value
                .as_bytes()
                .get(2)
                .is_some_and(|byte| matches!(byte, b'/' | b'\\'))
}

pub use super::ipc::*;
