use serde_json::{Value, json};

use crate::acp::{
    contract::{ItemLifecycle, RequiredText, SafeText, ToolState},
    intent::{ExternalId, NormalizedPayload},
    limits::LIMITS,
};

use super::{OmpTranslator, TranslationError, requests::encode_line};
use crate::acp::bindings::omp::{
    protocol::{AdapterEvent, ItemUpdateMode, Outbound, PermissionResponse},
    translator::updates::{category_label, external_id, object, tool_category},
    wire::opaque_id,
};

const MAX_PERMISSION_REQUESTS: usize = 8;

#[derive(Debug)]
pub(super) struct PermissionIntent {
    pub tool_call_id: ExternalId,
    pub raw_request_id: Value,
}

#[derive(Debug)]
pub(super) struct ResolvedPermission {
    pub tool_call_id: ExternalId,
    pub response_line: String,
    pub cancelled: bool,
}

impl OmpTranslator {
    pub(crate) fn permission_decline_prepared(
        &self,
        request_id: &str,
    ) -> Result<(Outbound, bool), TranslationError> {
        if let Some(resolved) = self.resolved_permissions.get(request_id) {
            return Ok((
                Outbound {
                    request_id: None,
                    line: resolved.response_line.clone(),
                },
                true,
            ));
        }
        let intent = self
            .pending_permissions
            .get(request_id)
            .ok_or(TranslationError::Correlation)?;
        Ok((
            Outbound {
                request_id: None,
                line: decline_line(&intent.raw_request_id)?,
            },
            false,
        ))
    }

    pub(super) fn translate_permission_request(
        &mut self,
        frame: &Value,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let params = object(frame.get("params"))?;
        self.validate_session(params.get("sessionId"))?;
        if self
            .active
            .as_ref()
            .is_some_and(|turn| turn.terminal.is_some())
        {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        let request_key = opaque_id(frame.get("id"))?;
        let tool = object(params.get("toolCall"))?;
        let tool_call_id = external_id(tool.get("toolCallId"))?;
        if let Some(existing) = self.resolved_permissions.get(&request_key) {
            return if existing.tool_call_id == tool_call_id {
                if existing.cancelled {
                    Ok(vec![AdapterEvent::IgnoredDuplicate])
                } else {
                    Ok(vec![AdapterEvent::PermissionDeclinePending {
                        request_id: RequiredText::new("requestId", request_key)?,
                        tool_call_id,
                    }])
                }
            } else {
                Err(TranslationError::Correlation)
            };
        }
        if let Some(existing) = self.pending_permissions.get(&request_key) {
            return if existing.tool_call_id == tool_call_id {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::Correlation)
            };
        }
        if self.pending_permissions.len() >= MAX_PERMISSION_REQUESTS
            || self.pending_permissions.len() as u64 >= LIMITS.queues.server_requests_per_session
        {
            return Err(TranslationError::Limit("permission requests"));
        }
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        let current = active.tools.get(tool_call_id.as_str()).copied();
        match current {
            Some(state) if state.is_terminal() => return Ok(vec![AdapterEvent::IgnoredLate]),
            Some(state) if state != ToolState::WaitingForApproval => {
                if !state.can_transition_to(ToolState::WaitingForApproval) {
                    return Err(TranslationError::ToolTransition);
                }
                active.tools.insert(
                    tool_call_id.as_str().to_owned(),
                    ToolState::WaitingForApproval,
                );
            }
            Some(_) => {}
            None => {
                if active.tools.len() as u64 >= LIMITS.wire.items_per_turn {
                    return Err(TranslationError::Limit("tools per Turn"));
                }
                active.tools.insert(
                    tool_call_id.as_str().to_owned(),
                    ToolState::WaitingForApproval,
                );
            }
        }
        let raw_request_id = frame
            .get("id")
            .cloned()
            .ok_or(TranslationError::MalformedFrame)?;
        self.pending_permissions.insert(
            request_key.clone(),
            PermissionIntent {
                tool_call_id: tool_call_id.clone(),
                raw_request_id,
            },
        );
        let category = tool_category(tool);
        Ok(vec![
            AdapterEvent::Item {
                external_key: RequiredText::new(
                    "externalItemId",
                    format!("permission:{request_key}"),
                )?,
                lifecycle: ItemLifecycle::Streaming,
                tool_state: Some(ToolState::WaitingForApproval),
                payload: NormalizedPayload::PermissionRequest {
                    category,
                    summary: SafeText::new(
                        "permissionSummary",
                        format!(
                            "{} permission requested; Sage policy requires denial",
                            category_label(category)
                        ),
                    )?,
                },
                update_mode: ItemUpdateMode::Replace,
            },
            AdapterEvent::PermissionDeclinePending {
                request_id: RequiredText::new("requestId", request_key)?,
                tool_call_id,
            },
        ])
    }

    /// Construct the deny response only after the durable decline fact commits.
    pub(crate) fn permission_decline_committed(
        &mut self,
        request_id: &str,
    ) -> Result<PermissionResponse, TranslationError> {
        if let Some(resolved) = self.resolved_permissions.get(request_id) {
            return Ok(PermissionResponse {
                outbound: Outbound {
                    request_id: None,
                    line: resolved.response_line.clone(),
                },
                event: AdapterEvent::IgnoredDuplicate,
            });
        }
        let intent = self
            .pending_permissions
            .remove(request_id)
            .ok_or(TranslationError::Correlation)?;
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        let current = active
            .tools
            .get(intent.tool_call_id.as_str())
            .copied()
            .ok_or(TranslationError::Correlation)?;
        if current != ToolState::WaitingForApproval {
            return Err(TranslationError::ToolTransition);
        }
        active
            .tools
            .insert(intent.tool_call_id.as_str().to_owned(), ToolState::Declined);
        let response_line = decline_line(&intent.raw_request_id)?;
        if self.resolved_permissions.len() == MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        self.resolved_permissions.insert(
            request_id.to_owned(),
            ResolvedPermission {
                tool_call_id: intent.tool_call_id.clone(),
                response_line: response_line.clone(),
                cancelled: false,
            },
        );
        Ok(PermissionResponse {
            outbound: Outbound {
                request_id: None,
                line: response_line,
            },
            event: AdapterEvent::Item {
                external_key: intent.tool_call_id,
                lifecycle: ItemLifecycle::Completed,
                tool_state: Some(ToolState::Declined),
                payload: NormalizedPayload::ToolUpdateResult {
                    category: crate::acp::intent::ToolCategory::Other,
                    label: SafeText::new("toolLabel", "OMP tool")?,
                    output_summary: Some(SafeText::new("toolOutput", "Permission declined")?),
                    locations: Default::default(),
                    command_activity: Default::default(),
                },
                update_mode: ItemUpdateMode::Replace,
            },
        })
    }

    pub(crate) fn permission_cancel_prepared(
        &self,
        request_id: &str,
    ) -> Result<Outbound, TranslationError> {
        let intent = self
            .pending_permissions
            .get(request_id)
            .ok_or(TranslationError::Correlation)?;
        Ok(Outbound {
            request_id: None,
            line: decline_line(&intent.raw_request_id)?,
        })
    }

    pub(crate) fn permission_cancel_committed(
        &mut self,
        request_id: &str,
    ) -> Result<(), TranslationError> {
        let intent = self
            .pending_permissions
            .remove(request_id)
            .ok_or(TranslationError::Correlation)?;
        let active = self.active.as_mut().ok_or(TranslationError::StaleIntent)?;
        let current = active
            .tools
            .get(intent.tool_call_id.as_str())
            .copied()
            .ok_or(TranslationError::Correlation)?;
        if current != ToolState::WaitingForApproval {
            return Err(TranslationError::ToolTransition);
        }
        active.tools.insert(
            intent.tool_call_id.as_str().to_owned(),
            ToolState::Cancelled,
        );
        if self.resolved_permissions.len() == MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        let response_line = decline_line(&intent.raw_request_id)?;
        self.resolved_permissions.insert(
            request_id.to_owned(),
            ResolvedPermission {
                tool_call_id: intent.tool_call_id,
                response_line,
                cancelled: true,
            },
        );
        Ok(())
    }
}

fn decline_line(raw_request_id: &Value) -> Result<String, TranslationError> {
    encode_line(json!({
        "jsonrpc": "2.0",
        "id": raw_request_id,
        "result": {"outcome": {"outcome": "cancelled"}}
    }))
}
