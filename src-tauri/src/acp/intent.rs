use serde::{Deserialize, Deserializer, Serialize, de};

use super::contract::{BoundedVec, ContractError, ItemKind, RequiredText, SafeText, SageRootId};
pub use super::{ipc::*, limits::COMMAND_RESOURCE_LIMIT};

mod activity_provenance;
mod capability;
mod error_kind;
mod file_change;
pub(crate) mod session_event;
mod tool_category;
mod tool_execution;
pub mod user_input;

pub use activity_provenance::{ActivityProvenanceV1, ActivitySourceKind};
pub use capability::CapabilitySnapshotV1;
pub use error_kind::ErrorKind;
pub(crate) use file_change::FileChangeFacts;
pub use file_change::FileChangeOperation;
pub use tool_category::ToolCategory;
pub use tool_execution::ToolExecution;

pub type BindingId = RequiredText<512>;
pub type ModelId = RequiredText<512>;
pub type ExternalId = RequiredText<512>;
pub type PageCursor = RequiredText<512>;
pub use super::transcript_quote::Prompt;

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_resource: Option<SafeResourceRef>,
    },
}

impl SafeResourceLocation {
    pub(crate) fn unavailable_with_display(
        reason: UnavailableResourceReason,
        display_resource: SafeResourceRef,
    ) -> Self {
        if reason != UnavailableResourceReason::Missing
            || display_resource.relative_path.as_str().len() > 512
            || display_resource
                .relative_path
                .as_str()
                .chars()
                .any(char::is_control)
        {
            return Self::unavailable(reason);
        }
        let Self::Unavailable { label, .. } = Self::unavailable(reason) else {
            unreachable!()
        };
        Self::Unavailable {
            reason,
            label,
            display_resource: Some(display_resource),
        }
    }

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
            display_resource: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageMediaType {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageSource {
    User,
    #[default]
    Agent,
}

impl ImageMediaType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            "image/webp" => Some(Self::Webp),
            _ => None,
        }
    }
}

/// Decoded raster cap. Stored JSON must also fit `LIMITS.wire.normalized_item_bytes`.
pub const IMAGE_DECODED_BYTES_MAX: usize = 262_144;
/// Standard base64 length for `IMAGE_DECODED_BYTES_MAX` (256 KiB * 4/3).
pub const IMAGE_BASE64_MAX: usize = 349_544;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NormalizedPlanState {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NormalizedPlanEntry {
    pub label: SafeText<512>,
    pub state: NormalizedPlanState,
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
    PlanEntries {
        entries: BoundedVec<NormalizedPlanEntry, 128>,
    },
    CompactionNotice,
    ToolCall {
        category: ToolCategory,
        label: SafeText<512>,
        input_summary: Option<SafeText<65536>>,
        #[serde(default)]
        output_summary: Option<SafeText<65536>>,
        #[serde(default)]
        locations: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        /// Legacy rows decode as ordinary commands.
        #[serde(default)]
        command_activity: crate::acp::declared_test::CommandActivity,
        #[serde(default)]
        execution: ToolExecution,
    },
    FileChangeDiff {
        resource: SafeResourceLocation,
        summary: SafeText<65536>,
        additions: Option<u64>,
        deletions: Option<u64>,
        /// Truthful overflow marker. Over-cap admits summary-only metadata;
        /// no partial side is ever retained.
        #[serde(default)]
        truncated: bool,
        /// Immutable snapshot revision from `acp_file_change_snapshots`.
        /// Zero with `pair_available == false` means summary-only.
        #[serde(default)]
        snapshot_revision: u64,
        /// Raw UTF-8 byte lengths of the admitted pair (pre-serialization).
        #[serde(default)]
        old_bytes: u64,
        #[serde(default)]
        new_bytes: u64,
        /// Empty unless `pair_available`; then the 64-char pair digest.
        #[serde(default)]
        content_digest: String,
        /// False for legacy summary-only rows, which decode as unavailable.
        #[serde(default)]
        pair_available: bool,
        #[serde(default)]
        operation: FileChangeOperation,
    },
    Image {
        media_type: ImageMediaType,
        data: SafeText<349544>,
        #[serde(default)]
        source: ImageSource,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context_window: Option<u32>,
    },
    PermissionRequest {
        category: ToolCategory,
        summary: SafeText<65536>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        offered_option_kinds: Option<BoundedVec<PermissionOptionKind, 8>>,
        /// The harness's own explanation for the request, when its protocol
        /// carries one. Absent means the harness gave none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<SafeText<2048>>,
        /// Present when the harness asks the user questions rather than for
        /// approval; the request is then answered, not allowed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_input: Option<Box<user_input::UserInputRequest>>,
    },
    Error {
        #[serde(
            default,
            deserialize_with = "error_kind::deserialize_optional",
            skip_serializing_if = "Option::is_none"
        )]
        kind: Option<ErrorKind>,
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
        #[serde(default)]
        operation: Option<SafeText<32>>,
        #[serde(default)]
        agent_key: Option<SafeText<512>>,
        #[serde(default)]
        agent_keys: BoundedVec<SafeText<512>, 32>,
        #[serde(default)]
        agent_states: BoundedVec<(SafeText<512>, SafeText<32>), 32>,
        /// Model and effort a spawn requested, only as the wire supplied them.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<SafeText<256>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_effort: Option<SafeText<32>>,
    },
}

impl NormalizedPayload {
    /// Summary-only file change: no pair is retained. `truncated` marks an
    /// over-cap admission; legacy rows decode with `truncated == false` and
    /// render as typed unavailable, never fabricated text.
    pub(crate) fn file_summary_only(facts: FileChangeFacts, truncated: bool) -> Self {
        Self::FileChangeDiff {
            resource: facts.resource,
            summary: facts.summary,
            additions: facts.additions,
            deletions: facts.deletions,
            truncated,
            snapshot_revision: 0,
            old_bytes: 0,
            new_bytes: 0,
            content_digest: String::new(),
            pair_available: false,
            operation: facts.operation,
        }
    }

    /// Admitted pair metadata. The texts live as BLOBs in
    /// `acp_file_change_snapshots`; only lengths and digest stay inline.
    pub(crate) fn file_admitted(
        facts: FileChangeFacts,
        snapshot_revision: u64,
        side_bytes: (u64, u64),
        content_digest: String,
    ) -> Self {
        Self::FileChangeDiff {
            resource: facts.resource,
            summary: facts.summary,
            additions: facts.additions,
            deletions: facts.deletions,
            truncated: false,
            snapshot_revision,
            old_bytes: side_bytes.0,
            new_bytes: side_bytes.1,
            content_digest,
            pair_available: true,
            operation: facts.operation,
        }
    }

    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Text { .. } => ItemKind::Text,
            Self::ThoughtSummary { .. } => ItemKind::ThoughtSummary,
            Self::Plan { .. } | Self::PlanEntries { .. } => ItemKind::Plan,
            Self::CompactionNotice => ItemKind::CompactionNotice,
            Self::ToolCall { .. } => ItemKind::ToolCall,
            Self::FileChangeDiff { .. } => ItemKind::FileChangeDiff,
            Self::Image { .. } => ItemKind::Image,
            Self::Usage { .. } => ItemKind::Usage,
            Self::PermissionRequest { .. } => ItemKind::PermissionRequest,
            Self::Error { .. } => ItemKind::Error,
            Self::Unsupported { .. } => ItemKind::Unsupported,
            Self::Collaboration { .. } => ItemKind::Collaboration,
        }
    }
}

#[cfg(test)]
mod tests;
