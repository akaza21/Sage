use serde_json::Value;

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterEvent as SharedEvent, AdapterRuntimeError,
        ResourceMapper as SharedResourceMapper,
    },
    contract::SafeText,
    intent::{NormalizedPayload, ToolCategory},
    lifecycle::{ToolState, TurnState},
};

use super::super::super::translator::AdapterEvent as StandardEvent;
use super::super::{OpenCodeRuntime, PermissionReply, PermissionState, map_translation};
use super::created_file::attach_completed_created_file;
use super::{MapperBridge, unsupported};

impl OpenCodeRuntime {
    pub(in super::super) fn ingest_notification(
        &mut self,
        mut value: Value,
        resources: &dyn SharedResourceMapper,
        request_digest: Option<[u8; 32]>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if self.safety_alerted {
            return AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new());
        }
        if value.get("id").is_some() {
            if value.get("method").and_then(Value::as_str) != Some("session/request_permission") {
                return self.ingest_client_fs_request(value);
            }
            return self.ingest_permission_request(
                value,
                request_digest.ok_or(AdapterRuntimeError::Frame)?,
            );
        }
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or(AdapterRuntimeError::Frame)?;
        if method != "session/update" {
            return AdapterBatch::new(
                vec![SharedEvent::Unsupported(unsupported(method)?)],
                Vec::new(),
            );
        }
        let turn_request_id = self
            .active
            .as_ref()
            .and_then(|active| active.turn.as_ref())
            .map(|turn| turn.request_id);
        attach_completed_created_file(&mut value, &self.permissions, turn_request_id);
        let active = self.active.as_mut().ok_or(AdapterRuntimeError::Frame)?;
        let events = active
            .translator
            .translate_notification_value(value, &MapperBridge(resources))
            .map_err(map_translation)?;
        let access_mode = active.turn.as_ref().map(|turn| turn.access_mode);
        let unexpected_completion = events.iter().any(|event| {
            let StandardEvent::Item {
                external_key,
                tool_state: Some(ToolState::Completed),
                payload: NormalizedPayload::ToolCall { category, .. },
                ..
            } = event
            else {
                return false;
            };
            if matches!(
                category,
                ToolCategory::Read | ToolCategory::Search | ToolCategory::Think
            ) {
                return false;
            }
            match access_mode {
                Some(crate::settings_store::SessionAccessMode::ReadOnly) => true,
                Some(crate::settings_store::SessionAccessMode::WorkspaceWrite)
                | Some(crate::settings_store::SessionAccessMode::Ask) => {
                    !self.permissions.values().any(|record| {
                        record.tool_id == external_key.as_str()
                            && matches!(
                                record.reply,
                                PermissionReply::AllowOnce | PermissionReply::AllowAlways
                            )
                            && record.state == PermissionState::Written
                    })
                }
                _ => false,
            }
        });
        if unexpected_completion {
            let cancel = self.cancel()?;
            self.safety_alerted = true;
            self.close_pending = true;
            let (code, message) = match access_mode {
                Some(crate::settings_store::SessionAccessMode::WorkspaceWrite) => (
                    "workspaceWriteSafetyViolation",
                    "OpenCode completed a non-read tool without Sage approval in WorkspaceWrite. The Turn was stopped; inspect the Project for changes.",
                ),
                Some(crate::settings_store::SessionAccessMode::Ask) => (
                    "askSafetyViolation",
                    "OpenCode completed a non-read tool without Sage approval in Ask mode. The Turn was stopped; inspect the Project for changes.",
                ),
                _ => (
                    "readOnlySafetyViolation",
                    "OpenCode completed a non-read tool without Sage approval in Read-only. The Turn was stopped; inspect the Project for changes.",
                ),
            };
            return AdapterBatch::new(
                vec![
                    SharedEvent::Error(NormalizedPayload::Error {
                        kind: None,
                        code: SafeText::new("errorCode", code)
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                        message: SafeText::new("errorMessage", message)
                            .map_err(|_| AdapterRuntimeError::Frame)?,
                        retryable: false,
                    }),
                    SharedEvent::Turn {
                        id: self.turn_identity()?,
                        state: TurnState::Failed,
                    },
                ],
                vec![cancel],
            );
        }
        let mut all = std::mem::take(&mut self.deferred);
        all.extend(events);
        self.translate(all)
    }
}
