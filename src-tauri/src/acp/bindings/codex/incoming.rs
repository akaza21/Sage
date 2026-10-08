use serde_json::{Map, Value};

use crate::acp::{
    contract::RequiredText,
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    limits::LIMITS,
};

use super::{
    request::RequestKind,
    state::{CodexAdapter, ScopedExternalId},
};

pub(super) mod support;
use support::{
    capability, content, error_payload, identity, parse_models, permission, text, unsupported,
};
pub(in crate::acp::bindings::codex) use support::{command_output_token, sanitize, validate_shape};

mod items;
use items::{normalize_item, normalize_materialized_command};

mod requests;
use requests::server_request;

mod notifications;
use notifications::notification;

mod response;
use response::{browser_mcp_unavailable, response};

pub(crate) trait ResourceMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;

    fn map_missing_display(&self, _absolute_path: &str) -> Option<SafeResourceRef> {
        None
    }
}

pub(in crate::acp::bindings::codex) const UNPROJECTED_DIAGNOSTIC: &str =
    "Codex emitted a message Sage does not project";

pub(in crate::acp::bindings::codex) fn record_unprojected(method: &str) {
    tracing::warn!(codex_method = %sanitize(method), "{UNPROJECTED_DIAGNOSTIC}");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ModelDescriptor {
    pub(crate) id: RequiredText<512>,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DenyResponse {
    pub(crate) external_request: ScopedExternalId,
    pub(crate) bytes: Vec<u8>,
}

pub(crate) use crate::acp::bindings::adapter::ItemUpdateMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IncomingEvent {
    Initialized {
        capabilities: crate::acp::intent::CapabilitySnapshotV1,
    },
    SessionReady {
        id: ScopedExternalId,
    },
    SessionLoaded {
        id: ScopedExternalId,
    },
    Models {
        models: Vec<ModelDescriptor>,
        next_cursor: Option<RequiredText<512>>,
    },
    Turn {
        id: ScopedExternalId,
        state: TurnState,
    },
    Item {
        id: ScopedExternalId,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
        update_mode: ItemUpdateMode,
    },
    ChildThreadItem {
        id: ScopedExternalId,
        lifecycle: ItemLifecycle,
        payload: NormalizedPayload,
        child_thread_id: crate::acp::intent::ExternalId,
    },
    Permission {
        turn_id: ScopedExternalId,
        item_id: ScopedExternalId,
        payload: NormalizedPayload,
        denial: Box<DenyResponse>,
    },
    Usage {
        turn_id: ScopedExternalId,
        payload: NormalizedPayload,
    },
    HostToolReply {
        bytes: Vec<u8>,
    },
    PermissionResolved {
        turn_id: ScopedExternalId,
        external_request: ScopedExternalId,
        state: crate::acp::lifecycle::ServerRequestState,
    },
    Acknowledged {
        closes_session: bool,
    },
    SteerResponse {
        request_id: u64,
        accepted: bool,
    },
    Unsupported(NormalizedPayload),
    Error(NormalizedPayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IngressDisposition {
    Events(Vec<IncomingEvent>),
    IgnoredLate,
    GoalMutationRejected,
    GoalMutationApplied,
    DuplicateTerminal,
    DuplicateServerRequest { denial: Option<Vec<u8>> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum IngressError {
    #[error("incoming frame exceeds its byte limit")]
    FrameBytes,
    #[error("incoming JSON is malformed or structurally over limit")]
    Malformed,
    #[error("incoming response is not correlated to an outstanding request")]
    Uncorrelated,
    #[error("incoming message has a conflicting terminal fact")]
    TerminalConflict,
    #[error("incoming message exceeds a normalized collection limit")]
    CollectionLimit,
    #[error("incoming message identity is invalid or foreign")]
    Identity,
    #[error("incoming normalized content is invalid")]
    Content,
    #[error("initialize response lacks the required pinned capability evidence")]
    CapabilityUnavailable,
    #[error("incoming server request is unsupported and requires runtime teardown")]
    UnsupportedServerRequest,
    #[error("permission decline state is conflicting or over limit and requires runtime teardown")]
    PermissionTeardownRequired,
}

pub(crate) fn ingest<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    bytes: &[u8],
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    ingest_inner(
        adapter,
        bytes,
        mapper,
        &crate::acp::declared_test::DeclaredTestCommands::default(),
    )
}

pub(crate) fn ingest_with_declarations<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    bytes: &[u8],
    mapper: &M,
    declarations: &crate::acp::declared_test::DeclaredTestCommands,
) -> Result<IngressDisposition, IngressError> {
    ingest_inner(adapter, bytes, mapper, declarations)
}

fn ingest_inner<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    bytes: &[u8],
    mapper: &M,
    declarations: &crate::acp::declared_test::DeclaredTestCommands,
) -> Result<IngressDisposition, IngressError> {
    if bytes.len() > LIMITS.wire.frame_bytes as usize {
        return Err(IngressError::FrameBytes);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| IngressError::Malformed)?;
    validate_shape(&value)?;
    if adapter.provisional_expired() {
        return Err(IngressError::Identity);
    }
    let object = value.as_object().ok_or(IngressError::Malformed)?;
    if object.contains_key("result") || object.contains_key("error") {
        return response(adapter, object);
    }
    let method = text(object, "method", LIMITS.wire.method_bytes as usize)?;
    let params = object
        .get("params")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    if object.contains_key("id") {
        return server_request(adapter, object, method, params);
    }
    let thread = params.get("threadId").and_then(Value::as_str).or_else(|| {
        (method == "thread/started")
            .then(|| params.get("thread")?.get("id")?.as_str())
            .flatten()
    });
    if thread.is_some_and(|thread| adapter.ignore_unbound_thread_notification(thread)) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    notification(adapter, method, params, mapper, declarations)
}
