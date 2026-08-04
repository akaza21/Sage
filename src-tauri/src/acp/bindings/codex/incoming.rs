use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, TerminalUpdate, ToolState, TurnState},
    limits::LIMITS,
};

use super::{
    request::RequestKind,
    state::{CodexAdapter, ExternalKind, ScopedExternalId},
};

#[path = "incoming_support.rs"]
mod support;
pub(super) use support::*;

#[path = "incoming_items.rs"]
mod items;
use items::normalize_item;

#[path = "incoming_requests.rs"]
mod requests;
use requests::server_request;

pub(crate) trait ResourceMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason>;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemUpdateMode {
    Append,
    Replace,
}

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
    Acknowledged {
        closes_session: bool,
    },
    Unsupported(NormalizedPayload),
    Error(NormalizedPayload),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum IngressDisposition {
    Events(Vec<IncomingEvent>),
    IgnoredLate,
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
    if bytes.len() > LIMITS.wire.frame_bytes as usize {
        return Err(IngressError::FrameBytes);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| IngressError::Malformed)?;
    validate_shape(&value)?;
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
    notification(adapter, method, params, mapper)
}

fn response(
    adapter: &mut CodexAdapter,
    object: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let id = object
        .get("id")
        .and_then(Value::as_u64)
        .ok_or(IngressError::Malformed)?;
    let kind = adapter.take_pending(id).ok_or(IngressError::Uncorrelated)?;
    if let Some(error) = object.get("error") {
        adapter.fail_response(id, kind).map_err(permission)?;
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex request failed");
        return Ok(IncomingEvent::Error(error_payload("codexRpc", message)?).into());
    }
    let result = object
        .get("result")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let event = match kind {
        RequestKind::Initialize => IncomingEvent::Initialized {
            capabilities: adapter.accept_initialize(result).map_err(capability)?,
        },
        RequestKind::Start | RequestKind::Resume => {
            let thread = result
                .get("thread")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(thread, "id", 512)?;
            let id = adapter.bind_thread(external).map_err(identity)?;
            IncomingEvent::SessionReady { id }
        }
        RequestKind::Read => {
            let thread = result
                .get("thread")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(thread, "id", 512)?;
            let id = adapter.bind_thread(external).map_err(identity)?;
            IncomingEvent::SessionLoaded { id }
        }
        RequestKind::Models => {
            let (models, next_cursor) = parse_models(result)?;
            IncomingEvent::Models {
                models,
                next_cursor,
            }
        }
        RequestKind::TurnStart => {
            let turn = result
                .get("turn")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let external = text(turn, "id", 512)?;
            let (id, binding) = adapter
                .complete_turn_start(id, external)
                .map_err(identity)?;
            if binding == super::state::TurnBinding::PreviousTerminal {
                return Ok(IngressDisposition::DuplicateTerminal);
            }
            IncomingEvent::Turn {
                id,
                state: TurnState::Sending,
            }
        }
        RequestKind::Interrupt => IncomingEvent::Acknowledged {
            closes_session: false,
        },
        RequestKind::Unsubscribe => IncomingEvent::Acknowledged {
            closes_session: true,
        },
    };
    Ok(event.into())
}

fn notification<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    method: &str,
    params: &Map<String, Value>,
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    match method {
        "turn/started" => {
            adapter
                .expect_thread(text(params, "threadId", 512)?)
                .map_err(identity)?;
            let turn = params
                .get("turn")
                .and_then(Value::as_object)
                .ok_or(IngressError::Malformed)?;
            let (id, binding) = adapter
                .observe_turn_started(text(turn, "id", 512)?)
                .map_err(identity)?;
            if binding == super::state::TurnBinding::PreviousTerminal {
                return Ok(IngressDisposition::IgnoredLate);
            }
            Ok(IncomingEvent::Turn {
                id,
                state: TurnState::Running,
            }
            .into())
        }
        "turn/completed" => turn_completed(adapter, params),
        "item/started" => item_event(adapter, params, ItemLifecycle::Started, mapper),
        "item/completed" => item_event(adapter, params, ItemLifecycle::Completed, mapper),
        "item/agentMessage/delta" => delta_event(adapter, params, DeltaKind::Text),
        "item/reasoning/summaryTextDelta" => delta_event(adapter, params, DeltaKind::Thought),
        "item/plan/delta" => delta_event(adapter, params, DeltaKind::Plan),
        "thread/tokenUsage/updated" => {
            expect_thread_and_turn(adapter, params)?;
            let turn = text(params, "turnId", 512)?;
            Ok(IncomingEvent::Usage {
                turn_id: adapter
                    .scoped_id(ExternalKind::Turn, turn)
                    .map_err(identity)?,
                payload: parse_usage(params)?,
            }
            .into())
        }
        "error" => {
            let message = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex error");
            Ok(IncomingEvent::Error(error_payload("codex", message)?).into())
        }
        other => {
            Ok(IncomingEvent::Unsupported(unsupported(other, "Unsupported Codex event")?).into())
        }
    }
}

fn turn_completed(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
) -> Result<IngressDisposition, IngressError> {
    let thread_id = text(params, "threadId", 512)?;
    adapter.expect_thread(thread_id).map_err(identity)?;
    let turn = params
        .get("turn")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let external = text(turn, "id", 512)?;
    adapter.expect_turn(external).map_err(identity)?;
    let state = match text(turn, "status", 32)? {
        "completed" => TurnState::Completed,
        "interrupted" => TurnState::Interrupted,
        "failed" => TurnState::Failed,
        _ => return Err(IngressError::Malformed),
    };
    match adapter
        .accept_turn_terminal(external, state)
        .map_err(identity)?
    {
        TerminalUpdate::Apply => {
            let id = adapter
                .scoped_id(ExternalKind::Turn, external)
                .map_err(identity)?;
            Ok(IncomingEvent::Turn { id, state }.into())
        }
        TerminalUpdate::NoOp => Ok(IngressDisposition::DuplicateTerminal),
        TerminalUpdate::Conflict => Err(IngressError::TerminalConflict),
    }
}

fn item_event<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    requested: ItemLifecycle,
    mapper: &M,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let external = text(item, "id", 512)?;
    if adapter.turn_is_terminal(turn) && !adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let (payload, tool_state, terminal) = normalize_item(item, requested, mapper)?;
    let lifecycle = terminal.unwrap_or(requested);
    if lifecycle.is_terminal() {
        match adapter
            .accept_item_terminal(turn, external, lifecycle)
            .map_err(identity)?
        {
            TerminalUpdate::NoOp => return Ok(IngressDisposition::DuplicateTerminal),
            TerminalUpdate::Conflict => return Err(IngressError::TerminalConflict),
            TerminalUpdate::Apply => {}
        }
    }
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle,
        tool_state,
        payload,
        update_mode: ItemUpdateMode::Replace,
    }
    .into())
}

enum DeltaKind {
    Text,
    Thought,
    Plan,
}

fn delta_event(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    kind: DeltaKind,
) -> Result<IngressDisposition, IngressError> {
    let turn = expect_thread_and_turn(adapter, params)?;
    if adapter.turn_is_terminal(turn) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let external = text(params, "itemId", 512)?;
    if adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let value = sanitize(text(params, "delta", 262_144)?);
    let payload = match kind {
        DeltaKind::Text => NormalizedPayload::Text {
            text: SafeText::new("text", value).map_err(content)?,
        },
        DeltaKind::Thought => NormalizedPayload::ThoughtSummary {
            summary: SafeText::new("summary", value).map_err(content)?,
        },
        DeltaKind::Plan => NormalizedPayload::Plan {
            summary: SafeText::new("plan", value).map_err(content)?,
        },
    };
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    Ok(IncomingEvent::Item {
        id,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload,
        update_mode: ItemUpdateMode::Append,
    }
    .into())
}
