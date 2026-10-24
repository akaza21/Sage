use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
};

use super::super::sanitize::normalized_label_text;
use super::decode::{
    category_label, fits_permission_payload, object, object_value, offered_option_kinds, rpc_id,
    string, transition_tool,
};
use super::{AdapterEvent, StandardAcpTranslator, TranslationError, decode_frame};
use crate::acp::bindings::command_summary::{CommandSummaryError, sanitize_command_summary};

const MAX_PERMISSION_REQUESTS: usize = 8;
const COMMAND_TOO_LONG: &str = "command too long to display";
const COMMAND_UNDISPLAYABLE: &str = "command cannot be displayed safely";
const COMMAND_HIDDEN: &str =
    "command hidden: contains content Sage will not display; review before approving";

impl StandardAcpTranslator {
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
        let category = self.permission_category(tool_call_id.as_str(), tool);
        let command = permission_command(tool)?;
        let relabel = self.adopt_permission_label(tool_call_id.as_str(), tool);
        let restated = self.adopt_permission_input(tool_call_id.as_str(), command.as_ref());
        let summary = self.permission_summary(tool_call_id.as_str(), command)?;
        let offered_option_kinds = offered_option_kinds(params)?;
        self.tools.insert(
            tool_call_id.as_str().to_owned(),
            ToolState::WaitingForApproval,
        );
        self.permissions.insert(
            request_id.as_str().to_owned(),
            tool_call_id.as_str().to_owned(),
        );
        let mut events = Vec::with_capacity(2);
        let relabel = match relabel {
            Some(label) => Some(label),
            None if restated => Some(self.permission_card_label(tool_call_id.as_str(), category)?),
            None => None,
        };
        if let Some(label) = relabel {
            events.push(self.tool_relabel_event(tool_call_id.as_str(), category, label)?);
        }
        let permission_key = format!("permission:{}", request_id.as_str());
        let permission_key = if permission_key.len() <= 512 {
            permission_key
        } else {
            format!(
                "permission:sha256:{:x}",
                Sha256::digest(request_id.as_str().as_bytes())
            )
        };
        events.push(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", permission_key)?,
            lifecycle: ItemLifecycle::Streaming,
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::PermissionRequest {
                category,
                summary,
                offered_option_kinds,
                reason: None,
            },
        });
        Ok(events)
    }

    fn adopt_permission_label(
        &mut self,
        tool_call_id: &str,
        tool: &Map<String, Value>,
    ) -> Option<SafeText<512>> {
        if self
            .tool_labels
            .get(tool_call_id)
            .is_some_and(|label| !self.generic_permission_label(label.as_str()))
        {
            return None;
        }
        let label = tool
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
            .and_then(|title| normalized_label_text::<512>("toolLabel", title).ok())?;
        self.tool_labels
            .insert(tool_call_id.to_owned(), label.clone());
        Some(label)
    }

    fn tool_relabel_event(
        &self,
        tool_call_id: &str,
        category: ToolCategory,
        label: SafeText<512>,
    ) -> Result<AdapterEvent, TranslationError> {
        let (output_summary, locations) = self.command_snapshot(tool_call_id);
        let execution =
            self.tool_execution(category, output_summary.as_ref().map(SafeText::as_str))?;
        Ok(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", tool_call_id)?,
            lifecycle: ItemLifecycle::Streaming,
            tool_state: Some(ToolState::WaitingForApproval),
            payload: NormalizedPayload::ToolCall {
                execution,
                category,
                label,
                input_summary: self.tool_inputs.get(tool_call_id).cloned(),
                output_summary,
                locations,
                command_activity: Default::default(),
            },
        })
    }

    fn permission_category(
        &mut self,
        tool_call_id: &str,
        tool: &Map<String, Value>,
    ) -> ToolCategory {
        let requested = tool
            .get("kind")
            .and_then(Value::as_str)
            .map(ToolCategory::from_wire)
            .filter(|category| *category != ToolCategory::Other);
        let Some(requested) = requested else {
            if tool
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| title.eq_ignore_ascii_case("PowerShell"))
            {
                self.tool_categories
                    .insert(tool_call_id.to_owned(), ToolCategory::Execute);
                return ToolCategory::Execute;
            }
            return self
                .tool_categories
                .get(tool_call_id)
                .copied()
                .unwrap_or(ToolCategory::Other);
        };
        self.tool_categories
            .insert(tool_call_id.to_owned(), requested);
        requested
    }

    fn permission_card_label(
        &self,
        tool_call_id: &str,
        category: ToolCategory,
    ) -> Result<SafeText<512>, TranslationError> {
        match self.tool_labels.get(tool_call_id) {
            Some(label) => Ok(label.clone()),
            None => SafeText::new("toolLabel", category_label(self.profile, category))
                .map_err(Into::into),
        }
    }

    fn generic_permission_label(&self, label: &str) -> bool {
        [
            ToolCategory::Read,
            ToolCategory::Edit,
            ToolCategory::Delete,
            ToolCategory::Move,
            ToolCategory::Search,
            ToolCategory::Execute,
            ToolCategory::Fetch,
            ToolCategory::WebSearch,
            ToolCategory::ImageView,
            ToolCategory::Think,
            ToolCategory::SwitchMode,
            ToolCategory::Mcp,
            ToolCategory::Subagent,
            ToolCategory::Other,
        ]
        .into_iter()
        .any(|category| label == category_label(self.profile, category))
    }

    fn adopt_permission_input(
        &mut self,
        tool_call_id: &str,
        command: Option<&SafeText<65536>>,
    ) -> bool {
        let Some(command) = command else {
            return false;
        };
        if self.tool_inputs.get(tool_call_id) == Some(command) {
            return false;
        }
        self.tool_inputs
            .insert(tool_call_id.to_owned(), command.clone());
        true
    }

    pub(super) fn host_settled_tool(&self, tool_call_id: &str) -> bool {
        self.resolved_permissions
            .values()
            .any(|settled| settled == tool_call_id)
    }

    fn settled_card_payload(
        &self,
        tool_call_id: &str,
        outcome: &'static str,
    ) -> Result<NormalizedPayload, TranslationError> {
        let category = self
            .tool_categories
            .get(tool_call_id)
            .copied()
            .unwrap_or(ToolCategory::Other);
        let (_, locations) = self.command_snapshot(tool_call_id);
        Ok(NormalizedPayload::ToolCall {
            execution: self.tool_execution(category, None)?,
            category,
            label: self.permission_card_label(tool_call_id, category)?,
            input_summary: self.tool_inputs.get(tool_call_id).cloned(),
            output_summary: Some(SafeText::new("toolOutput", outcome)?),
            locations,
            command_activity: Default::default(),
        })
    }

    fn permission_summary(
        &self,
        tool_call_id: &str,
        command: Option<SafeText<65536>>,
    ) -> Result<SafeText<65536>, TranslationError> {
        if let Some(command) = command {
            return Ok(command);
        }
        let summary = match self.tool_labels.get(tool_call_id) {
            Some(named) if !self.generic_permission_label(named.as_str()) => {
                SafeText::new("permissionSummary", named.as_str())?
            }
            _ => SafeText::new(
                "permissionSummary",
                format!(
                    "{} permission requested; awaiting your decision",
                    self.profile.other_tool_label
                ),
            )?,
        };
        Ok(summary)
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
        let settled = current.is_terminal();
        if !settled {
            transition_tool(current, ToolState::Declined)?;
            self.tools.insert(tool_id.clone(), ToolState::Declined);
        }
        if self.resolved_permissions.len() >= MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        self.resolved_permissions
            .insert(request_id.to_owned(), tool_id.clone());
        self.park_settled_diffs(&tool_id, ToolState::Declined)?;
        if settled {
            return Ok(AdapterEvent::IgnoredDuplicate);
        }
        let payload = self.settled_card_payload(&tool_id, "Permission declined")?;
        Ok(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", tool_id)?,
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Declined),
            payload,
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
        let settled = current.is_terminal();
        if !settled {
            transition_tool(current, ToolState::Cancelled)?;
            self.tools.insert(tool_id.clone(), ToolState::Cancelled);
        }
        if self.resolved_permissions.len() >= MAX_PERMISSION_REQUESTS
            && let Some(oldest) = self.resolved_permissions.keys().next().cloned()
        {
            self.resolved_permissions.remove(&oldest);
        }
        self.resolved_permissions
            .insert(request_id.to_owned(), tool_id.clone());
        self.park_settled_diffs(&tool_id, ToolState::Cancelled)?;
        if settled {
            return Ok(AdapterEvent::IgnoredDuplicate);
        }
        let payload = self.settled_card_payload(&tool_id, "Permission cancelled")?;
        Ok(AdapterEvent::Item {
            external_key: RequiredText::new("externalItemId", tool_id)?,
            lifecycle: ItemLifecycle::Completed,
            tool_state: Some(ToolState::Cancelled),
            payload,
        })
    }
}

fn permission_command(
    tool: &Map<String, Value>,
) -> Result<Option<SafeText<65536>>, TranslationError> {
    let Some(command) = tool
        .get("rawInput")
        .and_then(Value::as_object)
        .and_then(|input| input.get("command"))
        .and_then(Value::as_str)
        .filter(|command| !command.trim().is_empty())
    else {
        return Ok(None);
    };
    let shown = match displayable_command(command) {
        Ok(shown) => shown,
        Err(placeholder) => SafeText::new("permissionCommand", placeholder)?,
    };
    Ok(Some(shown))
}

fn displayable_command(command: &str) -> Result<SafeText<65536>, &'static str> {
    let shown = sanitize_command_summary(command).map_err(|error| match error {
        CommandSummaryError::TooLong => COMMAND_TOO_LONG,
        CommandSummaryError::Undisplayable => COMMAND_UNDISPLAYABLE,
        CommandSummaryError::Hidden => COMMAND_HIDDEN,
    })?;
    if !fits_permission_payload(shown.as_str()) {
        return Err(COMMAND_TOO_LONG);
    }
    Ok(shown)
}
