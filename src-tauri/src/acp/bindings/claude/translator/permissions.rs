use serde_json::Value;

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

use super::{
    AdapterEvent, ClaudeTranslator, TranslationError, decode_frame, object, object_value, rpc_id,
    string, transition_tool,
};

const MAX_PERMISSION_REQUESTS: usize = 8;

impl ClaudeTranslator {
    pub(crate) fn translate_permission_request(
        &mut self,
        raw: &[u8],
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let frame = decode_frame(raw)?;
        self.translate_permission_request_value(frame)
    }

    pub(crate) fn translate_permission_request_value(
        &mut self,
        frame: Value,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        if self.terminal.is_some() {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        if frame.get("method").and_then(Value::as_str) != Some("session/request_permission") {
            return Err(TranslationError::UnexpectedMethod);
        }
        let params = object(&frame, "params")?;
        self.validate_session(params.get("sessionId"))?;
        let request_id = rpc_id(frame.get("id"))?;
        let tool = object_value(params.get("toolCall"))?;
        let tool_call_id: RequiredText<512> =
            RequiredText::new("toolCallId", string(tool, "toolCallId")?)?;
        if let Some(existing) = self.resolved_permissions.get(request_id.as_str()) {
            return if existing == tool_call_id.as_str() {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::ForeignId)
            };
        }
        let current = self
            .tools
            .get(tool_call_id.as_str())
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        if current.is_terminal() {
            return Ok(vec![AdapterEvent::IgnoredLate]);
        }
        if let Some(existing) = self.permissions.get(request_id.as_str()) {
            return if existing == tool_call_id.as_str() {
                Ok(vec![AdapterEvent::IgnoredDuplicate])
            } else {
                Err(TranslationError::ForeignId)
            };
        }
        if self.permissions.len() >= MAX_PERMISSION_REQUESTS
            || self.permissions.len() as u64 >= LIMITS.queues.server_requests_per_session
        {
            return Err(TranslationError::PermissionLimit);
        }
        transition_tool(current, ToolState::WaitingForApproval)?;
        self.tools.insert(
            tool_call_id.as_str().to_owned(),
            ToolState::WaitingForApproval,
        );
        self.permissions.insert(
            request_id.as_str().to_owned(),
            tool_call_id.as_str().to_owned(),
        );
        Ok(vec![AdapterEvent::Item {
            external_key: RequiredText::new(
                "externalItemId",
                format!("permission:{}", request_id.as_str()),
            )?,
            lifecycle: ItemLifecycle::Streaming,
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::PermissionRequest {
                category: ToolCategory::Other,
                summary: SafeText::new(
                    "permissionSummary",
                    "Claude tool permission requested; awaiting your decision",
                )?,
            },
        }])
    }

    /// Called only after the durable decline fact has committed.
    pub(crate) fn permission_decline_committed(
        &mut self,
        request_id: &str,
    ) -> Result<AdapterEvent, TranslationError> {
        let tool_id = self
            .permissions
            .remove(request_id)
            .or_else(|| self.resolved_permissions.get(request_id).cloned())
            .ok_or(TranslationError::ForeignId)?;
        if self.resolved_permissions.contains_key(request_id) {
            return Ok(AdapterEvent::IgnoredDuplicate);
        }
        let current = self
            .tools
            .get(&tool_id)
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        transition_tool(current, ToolState::Declined)?;
        self.tools.insert(tool_id.clone(), ToolState::Declined);
        if self.resolved_permissions.len() >= MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        self.resolved_permissions
            .insert(request_id.to_owned(), tool_id.clone());
        Ok(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", tool_id)?,
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Declined),
            payload: NormalizedPayload::ToolUpdateResult {
                category: ToolCategory::Other,
                label: SafeText::new("toolLabel", "Claude tool")?,
                output_summary: Some(SafeText::new("toolOutput", "Permission declined")?),
                locations: Default::default(),
            },
        })
    }

    pub(crate) fn permission_cancel_committed(
        &mut self,
        request_id: &str,
    ) -> Result<AdapterEvent, TranslationError> {
        let tool_id = self
            .permissions
            .remove(request_id)
            .ok_or(TranslationError::ForeignId)?;
        let current = self
            .tools
            .get(&tool_id)
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        transition_tool(current, ToolState::Cancelled)?;
        self.tools.insert(tool_id.clone(), ToolState::Cancelled);
        if self.resolved_permissions.len() >= MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        self.resolved_permissions
            .insert(request_id.to_owned(), tool_id.clone());
        Ok(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", tool_id)?,
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Cancelled),
            payload: NormalizedPayload::ToolUpdateResult {
                category: ToolCategory::Other,
                label: SafeText::new("toolLabel", "Claude tool")?,
                output_summary: Some(SafeText::new("toolOutput", "Permission cancelled")?),
                locations: Default::default(),
            },
        })
    }
}
