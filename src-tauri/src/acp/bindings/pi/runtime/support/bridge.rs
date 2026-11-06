use super::super::PiRuntime;
use crate::acp::bindings::pi::{
    AdapterOutput,
    bridge::{Bridge, Pending, files, write},
    translator::PendingUiRequest,
};
use crate::{
    acp::{
        bindings::adapter::{
            AdapterBatch, AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressResolution,
            EgressToken, ResourceMapper,
        },
        contract::{SafeText, SageServerRequestId},
        file_diff::{PairAdmission, admit_pair},
        intent::{
            ExternalId, FileChangeFacts, FileChangeOperation, NormalizedPayload,
            SafeResourceLocation,
        },
        lifecycle::ItemLifecycle,
        runtime::permissions::PermissionDecision,
        transport::{Frame, OutboundFrame, request_payload_digest},
    },
    settings_store::SessionAccessMode,
};
use serde_json::{Value, json};

fn reply(id: &str, value: &str, token: EgressToken) -> Result<AdapterEgress, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(&json!({"type":"extension_ui_response", "id":id,"value":value}))
        .map_err(|_| AdapterRuntimeError::Frame)?;
    Ok(AdapterEgress::new(
        OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)?,
        token,
    ))
}

impl PiRuntime {
    pub(crate) fn enable_bridge(&mut self, workspace: crate::workspace::Workspace) {
        self.bridge = Some(Bridge::new(workspace));
        self.translator.bridge_tools = true;
    }

    pub(in crate::acp::bindings::pi::runtime) fn ingest_bridge(
        &mut self,
        value: &Value,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty() && id.len() <= 512)
            .ok_or(AdapterRuntimeError::Frame)?;
        crate::acp::bindings::pi::decode::validate_opaque_text(id, 512)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let refused = || {
            AdapterBatch::new(
                Vec::new(),
                vec![reply(id, "denied", EgressToken::protocol_notification())?],
            )
        };
        let title = value.get("title").and_then(Value::as_str).unwrap_or("");
        let body = value
            .get("placeholder")
            .and_then(Value::as_str)
            .unwrap_or("");
        if value.get("method").and_then(Value::as_str) != Some("input")
            || self.active_turn.is_none()
            || self.poisoned
        {
            return refused();
        }
        let Some(bridge) = self.bridge.as_mut() else {
            return refused();
        };
        if bridge.mode == SessionAccessMode::ReadOnly || bridge.writes >= 32 {
            return refused();
        }
        if title == "sage.pi.prepare.v1" {
            if bridge.pending.is_some() || self.deny.is_some() || body.len() > files::MAX_BYTES {
                return refused();
            }
            let request = match serde_json::from_str::<files::Request>(body) {
                Ok(request) => request,
                Err(_) => return refused(),
            };
            // Bind the request to an observed, current tool, never a free-standing filesystem RPC.
            if !self
                .active_turn
                .as_ref()
                .is_some_and(|turn| turn.tools.contains_key(&request.tool_call_id))
            {
                return refused();
            }
            if bridge.used.len() >= 32 || bridge.used.contains(&request.tool_call_id) {
                return refused();
            }
            let edit = match files::prepare(&bridge.workspace, request) {
                Ok(edit) => edit,
                Err(_) => return refused(),
            };
            bridge.used.insert(edit.tool_id.clone());
            bridge.pending = Some(Pending {
                request_id: id.into(),
                edit,
                approved: false,
                written: false,
            });
            self.translator.remember_pending_ui(PendingUiRequest {
                id: id.into(),
                method: "sage-write".into(),
                options: Vec::new(),
            });
            return self.translate_outputs(
                vec![AdapterOutput::PermissionRequested {
                    external_id: id.into(),
                }],
                None,
                request_payload_digest(frame).map_err(|_| AdapterRuntimeError::Frame)?,
                resources,
            );
        }
        if title != "sage.pi.commit.v1" {
            return refused();
        }
        let Some(pending) = bridge.pending.take() else {
            return refused();
        };
        if !pending.approved || !pending.written || pending.edit.tool_id != body {
            return refused();
        }
        if !self
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.tools.contains_key(&pending.edit.tool_id))
        {
            return refused();
        }
        let edit = pending.edit;
        let root_id = crate::acp::contract::SageRootId::parse(bridge.workspace.root_id())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let resource = crate::acp::intent::SafeResourceRef::new(root_id, edit.relative.clone())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        if write::commit(&bridge.workspace, &edit).is_err() {
            return refused();
        }
        bridge.writes += 1;
        let PairAdmission::Admitted {
            old_bytes,
            new_bytes,
            digest,
        } = admit_pair(&edit.old, &edit.new)
        else {
            return Err(AdapterRuntimeError::MandatoryDenyTeardown);
        };
        let external = ExternalId::new("piFileId", format!("file-{}", edit.tool_id))
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let facts = FileChangeFacts {
            resource: SafeResourceLocation::Resource { resource },
            summary: SafeText::new("fileSummary", "Pi updated a workspace file")
                .map_err(|_| AdapterRuntimeError::Frame)?,
            additions: None,
            deletions: None,
            operation: if edit.created {
                FileChangeOperation::Created
            } else {
                FileChangeOperation::Updated
            },
        };
        let payload = NormalizedPayload::file_admitted(
            facts,
            0,
            (old_bytes as u64, new_bytes as u64),
            digest,
        );
        let item = self.item(external.clone(), ItemLifecycle::Completed, None, payload)?;
        let turn = self.turn_identity()?;
        let pair = AdapterEvent::FilePair {
            id: self.identity(
                external.clone(),
                Some(turn.external_id),
                turn.turn_client_message_id,
            ),
            external_item_id: external,
            old_text: edit.old,
            new_text: edit.new,
        };
        AdapterBatch::new(
            vec![item, pair],
            vec![reply(id, "written", EgressToken::protocol_notification())?],
        )
    }

    pub(in crate::acp::bindings::pi::runtime) fn bridge_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let Some(pending) = self
            .bridge
            .as_mut()
            .and_then(|b| b.pending.as_mut())
            .filter(|p| p.request_id == request_id.as_str())
        else {
            return Ok(None);
        };
        if pending.approved || !self.translator.permission_waiting_for(request_id.as_str()) {
            return Err(AdapterRuntimeError::LaterPermissionMiss);
        }
        if !matches!(
            decision,
            PermissionDecision::AllowOnce | PermissionDecision::Deny
        ) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let allowed = decision == PermissionDecision::AllowOnce;
        pending.approved = allowed;
        self.translator.take_pending_ui();
        let response = reply(
            request_id.as_str(),
            if allowed { "approved" } else { "denied" },
            EgressToken::protocol_notification(),
        )?;
        crate::acp::runtime::later_permission::resolve_later_permission(
            request_id,
            decision,
            Some(response),
        )
        .map_err(|_| AdapterRuntimeError::LaterPermissionMiss)
    }

    pub(in crate::acp::bindings::pi::runtime) fn resolve_bridge_egress(
        &mut self,
        token: &EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        let Some(bridge) = self.bridge.as_mut() else {
            return Ok(());
        };
        if let Some(id) = token.allow_request()
            && let Some(pending) = bridge.pending.as_mut().filter(|p| p.request_id == id)
        {
            if resolution == EgressResolution::Written {
                pending.written = true;
            }
            if matches!(
                resolution,
                EgressResolution::QueueRejected
                    | EgressResolution::WriteZeroBytes
                    | EgressResolution::DeliveryUnknown
            ) {
                bridge.pending = None;
            }
        }
        if token.deny_request().is_some() {
            bridge.pending = None;
        }
        Ok(())
    }
}
