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

#[path = "incoming_support.rs"]
mod support;
pub(in crate::acp::bindings::codex) use support::*;

#[path = "incoming_items.rs"]
mod items;
use items::normalize_item;

#[path = "incoming_requests.rs"]
mod requests;
use requests::server_request;

#[path = "incoming_notifications.rs"]
mod notifications;
use notifications::notification;

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
    let kind = adapter.pending_kind(id).ok_or(IngressError::Uncorrelated)?;
    if let Some(error) = object.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Codex request failed");
        let payload = error_payload("codexRpc", message, false)?;
        if kind != RequestKind::TurnStart {
            let removed = adapter.take_pending(id);
            debug_assert_eq!(removed, Some(kind));
        }
        return match kind {
            RequestKind::TurnStart => match adapter.fail_turn_start(id).map_err(identity)? {
                super::state::TurnStartFailure::Failed(turn) => {
                    Ok(IngressDisposition::Events(vec![
                        IncomingEvent::Error(payload),
                        IncomingEvent::Turn {
                            id: turn,
                            state: TurnState::Failed,
                        },
                    ]))
                }
                super::state::TurnStartFailure::Late => Ok(IngressDisposition::DuplicateTerminal),
            },
            RequestKind::Interrupt => {
                let turn = adapter.fail_interrupt().map_err(identity)?;
                Ok(IngressDisposition::Events(vec![
                    IncomingEvent::Error(payload),
                    IncomingEvent::Turn {
                        id: turn,
                        state: TurnState::Failed,
                    },
                ]))
            }
            RequestKind::Unsubscribe => Ok(IngressDisposition::Events(vec![
                IncomingEvent::Error(payload),
                IncomingEvent::Acknowledged {
                    closes_session: false,
                },
            ])),
            _ => Ok(IncomingEvent::Error(payload).into()),
        };
    }
    let result = object
        .get("result")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    if kind != RequestKind::TurnStart {
        let removed = adapter.take_pending(id);
        debug_assert_eq!(removed, Some(kind));
    }
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
            match binding {
                super::state::TurnBinding::PreviousTerminal => {
                    return Ok(IngressDisposition::DuplicateTerminal);
                }
                super::state::TurnBinding::Active => {
                    return Ok(IngressDisposition::IgnoredLate);
                }
                super::state::TurnBinding::NewlyActive => {}
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
