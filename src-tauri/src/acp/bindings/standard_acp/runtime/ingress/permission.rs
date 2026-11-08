use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterEvent as SharedEvent, AdapterRuntimeError},
    intent::{ExternalId, NormalizedPayload},
};

use super::super::super::translator::AdapterEvent as StandardEvent;
use super::super::{
    PermissionIntent, PermissionReply, PermissionState, StandardAcpRuntime, outbound,
    permission_identity,
};
use super::created_file::created_file_content;
use super::path::{workspace_edit_path, workspace_edit_target};

impl StandardAcpRuntime {
    pub(super) fn ingest_permission_request(
        &mut self,
        value: Value,
        request_digest: [u8; 32],
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if value.get("method").and_then(Value::as_str) != Some("session/request_permission") {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        }
        let (wire_request_id, translator_id, wire_id) = permission_identity(value.get("id"))?;
        let request_id = super::super::scoped_permission_id(
            &self.session_id,
            self.runtime_epoch,
            &wire_request_id,
        )?;
        let tool_id = value
            .pointer("/params/toolCall/toolCallId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 512)
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?
            .to_owned();
        if let Some(existing) = self.permissions.get(request_id.as_str()) {
            if existing.request_digest != request_digest {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
            if existing.state == PermissionState::Written {
                let egress = permission_egress(
                    &request_id,
                    &existing.wire_id,
                    existing.reply,
                    true,
                    match existing.reply {
                        PermissionReply::AllowAlways => existing.allow_always_option_id.as_deref(),
                        _ => existing.allow_once_option_id.as_deref(),
                    },
                )?;
                return AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], vec![egress]);
            }
            return AdapterBatch::new(vec![SharedEvent::IgnoredDuplicate], Vec::new());
        }
        let allow_once_option_id = offered_option_id(&value, "allow_once");
        let allow_always_option_id = offered_option_id(&value, "allow_always");
        let active = self
            .active
            .as_mut()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
        let access_mode = active
            .turn
            .as_ref()
            .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?
            .access_mode;
        let turn_request_id = active.turn.as_ref().expect("active turn").request_id;
        let approved_path = workspace_edit_path(&value, &self.workspace_root);
        let created_file_content = created_file_content(&value, approved_path.as_deref());
        let reply = match access_mode {
            // Modes OpenCode's access policy never admits are denied too.
            crate::settings_store::SessionAccessMode::ReadOnly
            | crate::settings_store::SessionAccessMode::AcceptEdits
            | crate::settings_store::SessionAccessMode::Plan
            | crate::settings_store::SessionAccessMode::Auto
            | crate::settings_store::SessionAccessMode::FullAccess => {
                active
                    .translator
                    .translate_permission_request_value(value.clone())
                    .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
                PermissionReply::Deny
            }
            crate::settings_store::SessionAccessMode::WorkspaceWrite => {
                if allow_once_option_id.is_some()
                    && workspace_edit_target(&value, &self.workspace_root)
                {
                    PermissionReply::AllowOnce
                } else {
                    PermissionReply::AwaitUser
                }
            }
            crate::settings_store::SessionAccessMode::Ask => PermissionReply::AwaitUser,
        };
        if self.permissions.len() >= 8 {
            let retired = self
                .permissions
                .iter()
                .find(|(_, record)| record.state == PermissionState::Written)
                .map(|(key, _)| key.clone())
                .ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?;
            self.permissions.remove(&retired);
        }
        let mut published = Vec::new();
        if reply != PermissionReply::Deny {
            let translated = active
                .translator
                .translate_permission_request_value(value.clone())
                .map_err(|_| AdapterRuntimeError::MandatoryDenyTeardown)?;
            let mut saw_permission = false;
            for event in translated {
                match event {
                    StandardEvent::Item {
                        external_key,
                        lifecycle,
                        tool_state,
                        payload: payload @ NormalizedPayload::PermissionRequest { .. },
                    } => {
                        let _ = (external_key, lifecycle, tool_state);
                        saw_permission = true;
                        if reply == PermissionReply::AwaitUser {
                            published.push(SharedEvent::PermissionRequest {
                                turn_id: self.turn_identity()?,
                                item_id: self.item_identity(request_id.clone())?,
                                external_request_id: request_id.clone(),
                                payload,
                                request_payload_digest: request_digest,
                            });
                        }
                    }
                    StandardEvent::Item {
                        external_key,
                        lifecycle,
                        tool_state,
                        payload,
                    } => {
                        published.push(SharedEvent::Item {
                            id: self.item_identity(external_key)?,
                            lifecycle,
                            tool_state,
                            payload,
                        });
                    }
                    _ => return Err(AdapterRuntimeError::MandatoryDenyTeardown),
                }
            }
            if !saw_permission {
                return Err(AdapterRuntimeError::MandatoryDenyTeardown);
            }
        }
        let egress = if reply == PermissionReply::AwaitUser {
            Vec::new()
        } else {
            vec![permission_egress(
                &request_id,
                &wire_id,
                reply,
                false,
                allow_once_option_id.as_deref(),
            )?]
        };
        self.permissions.insert(
            request_id.as_str().to_owned(),
            PermissionIntent {
                translator_id,
                tool_id,
                request_digest,
                wire_id,
                allow_once_option_id,
                allow_always_option_id,
                reply,
                state: PermissionState::Pending,
                turn_request_id,
                approved_path,
                client_write_used: false,
                created_file_content,
                created_file_written: false,
            },
        );
        AdapterBatch::new(published, egress)
    }
}

fn offered_option_id(value: &Value, wanted: &str) -> Option<String> {
    value
        .pointer("/params/options")?
        .as_array()?
        .iter()
        .take(8)
        .find_map(|option| {
            let matching = option["kind"] == wanted
                || (option["kind"].as_str().is_none_or(str::is_empty)
                    && match wanted {
                        "allow_once" => {
                            matches!(option["name"].as_str(), Some("Allow" | "Allow once"))
                        }
                        "allow_always" => option["name"].as_str() == Some("Allow always"),
                        _ => false,
                    });
            if !matching {
                return None;
            }
            option
                .get("optionId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() <= 64)
        })
        .map(str::to_owned)
}

pub(in super::super) fn permission_egress(
    request_id: &ExternalId,
    wire_id: &Value,
    reply: PermissionReply,
    replay: bool,
    selected_option_id: Option<&str>,
) -> Result<crate::acp::bindings::adapter::AdapterEgress, AdapterRuntimeError> {
    let (outcome, token) = match reply {
        PermissionReply::Deny => (
            json!({"outcome":"cancelled"}),
            crate::acp::bindings::adapter::EgressToken::mandatory_deny(request_id.clone(), replay),
        ),
        PermissionReply::AllowOnce | PermissionReply::AllowAlways => (
            json!({"outcome":"selected","optionId":selected_option_id.ok_or(AdapterRuntimeError::MandatoryDenyTeardown)?}),
            crate::acp::bindings::adapter::EgressToken::permission_allow(request_id.clone()),
        ),
        PermissionReply::AwaitUser => return Err(AdapterRuntimeError::EgressIntent),
    };
    Ok(crate::acp::bindings::adapter::AdapterEgress::new(
        outbound(json!({"jsonrpc":"2.0","id":wire_id,"result":{"outcome":outcome}}))?,
        token,
    ))
}
