use super::super::{DenyStage, PiRuntime};
use crate::acp::bindings::pi::{
    AdapterOutput,
    bridge::{Bridge, Pending, Refusal, files, write},
    translator::{BRIDGE_WRITE_METHOD, PendingUiRequest},
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
        lifecycle::{ItemLifecycle, ServerRequestState},
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

    /// Grants the bridge write authority over the session's project root.
    /// Callers enable it only when the bundled extension is present in Pi.
    pub(crate) fn enable_bridge_for_root(
        &mut self,
        root: &crate::project_registry::ProjectRootAuthority,
    ) -> Result<(), AdapterRuntimeError> {
        let (project, _, root_id) = root.scope();
        let workspace =
            crate::workspace::Workspace::open(project.as_str(), root_id.as_str(), root.as_path())
                .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        self.enable_bridge(workspace);
        Ok(())
    }

    pub(in crate::acp::bindings::pi::runtime) fn begin_bridge_turn(
        &mut self,
        mode: SessionAccessMode,
    ) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.begin_turn(mode);
        }
    }

    /// Drops the retained old/new pair; a later commit then finds nothing to write.
    pub(in crate::acp::bindings::pi::runtime) fn discard_bridge_edit(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.pending = None;
        }
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
        let refused = |refusal: Refusal| {
            AdapterBatch::new(
                Vec::new(),
                vec![reply(
                    id,
                    refusal.message(),
                    EgressToken::protocol_notification(),
                )?],
            )
        };
        let title = value.get("title").and_then(Value::as_str).unwrap_or("");
        let body = value
            .get("placeholder")
            .and_then(Value::as_str)
            .unwrap_or("");
        if value.get("method").and_then(Value::as_str) != Some("input") {
            return refused(Refusal::Unsupported);
        }
        if self.active_turn.is_none() || self.poisoned {
            return refused(Refusal::Inactive);
        }
        let Some(bridge) = self.bridge.as_mut() else {
            return refused(Refusal::Unsupported);
        };
        if bridge.mode == SessionAccessMode::ReadOnly {
            return refused(Refusal::ReadOnly);
        }
        if bridge.writes >= 32 {
            return refused(Refusal::WriteLimit);
        }
        if title == "sage.pi.prepare.v1" {
            if body.len() > files::MAX_BYTES {
                return refused(Refusal::TooLarge);
            }
            let Ok(request) = serde_json::from_str::<files::Request>(body) else {
                return refused(Refusal::Unsupported);
            };
            // Each tool call gets one approval request, whatever its answer was.
            if bridge.used.contains(&request.tool_call_id) {
                return refused(Refusal::Replayed);
            }
            if bridge.pending.is_some() || self.deny.is_some() {
                return refused(Refusal::Busy);
            }
            // Bind the request to an observed, current tool, never a free-standing filesystem RPC.
            if !self
                .active_turn
                .as_ref()
                .is_some_and(|turn| turn.tools.contains_key(&request.tool_call_id))
            {
                return refused(Refusal::Unsupported);
            }
            if bridge.used.len() >= 32 {
                return refused(Refusal::WriteLimit);
            }
            let prepared = match files::prepare(&bridge.workspace, request) {
                Ok(prepared) => prepared,
                Err(refusal) => return refused(refusal),
            };
            bridge.used.insert(prepared.tool_id().to_owned());
            bridge.pending = Some(Pending {
                request_id: id.into(),
                prepared,
                approved: false,
                written: false,
            });
            self.translator.remember_pending_ui(PendingUiRequest {
                id: id.into(),
                method: BRIDGE_WRITE_METHOD.into(),
                options: Vec::new(),
                form: None,
                expires: false,
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
        if title == "sage.pi.withdraw.v1" {
            return self.withdraw_bridge_edit(id, body);
        }
        if title != "sage.pi.commit.v1" {
            return refused(Refusal::Unsupported);
        }
        // A commit names the one approved edit; anything else was never approved.
        let Some(pending) = bridge.pending.take() else {
            return refused(Refusal::NotApproved);
        };
        // A shell approval has nothing to commit: Pi runs the command itself.
        let files::Prepared::Edit(edit) = pending.prepared else {
            return refused(Refusal::NotApproved);
        };
        if !pending.approved || !pending.written || edit.tool_id != body {
            return refused(Refusal::NotApproved);
        }
        if !self
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.tools.contains_key(&edit.tool_id))
        {
            return refused(Refusal::Inactive);
        }
        let root_id = crate::acp::contract::SageRootId::parse(bridge.workspace.root_id())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let resource = crate::acp::intent::SafeResourceRef::new(root_id, edit.relative.clone())
            .map_err(|_| AdapterRuntimeError::Frame)?;
        if let Err(refusal) = write::commit(&bridge.workspace, &edit) {
            return refused(refusal);
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
        let (additions, deletions) = crate::acp::intent::line_delta(&edit.old, &edit.new);
        let verb = if edit.created { "Created" } else { "Edited" };
        let facts = FileChangeFacts {
            resource: SafeResourceLocation::Resource { resource },
            summary: SafeText::new("fileSummary", format!("{verb} {}", edit.relative))
                .map_err(|_| AdapterRuntimeError::Frame)?,
            additions: Some(additions),
            deletions: Some(deletions),
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

    /// Pi expires or aborts an extension dialog without telling Sage, so the
    /// bridge asks Sage to settle a prepare that got no answer. Sage alone
    /// decides: a recorded approval stands, an undecided request is cancelled
    /// durably and frees the turn's single pending slot, anything else is denied.
    fn withdraw_bridge_edit(
        &mut self,
        id: &str,
        tool_id: &str,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let answer = |value: &str| {
            AdapterBatch::new(
                Vec::new(),
                vec![reply(id, value, EgressToken::protocol_notification())?],
            )
        };
        let Some(pending) = self
            .bridge
            .as_ref()
            .and_then(|bridge| bridge.pending.as_ref())
            .filter(|pending| pending.prepared.tool_id() == tool_id)
        else {
            return answer(Refusal::Unsupported.message());
        };
        if pending.approved {
            return answer("approved");
        }
        let request_id = pending.request_id.clone();
        if !self.translator.permission_waiting_for(&request_id) {
            return answer(Refusal::Denied.message()); // A denial is already on its way.
        }
        self.discard_bridge_edit();
        self.translator.take_pending_ui();
        if self.deny.as_ref().is_some_and(|deny| {
            deny.request_id.as_str() == request_id && deny.stage == DenyStage::Prepared
        }) {
            self.deny = None;
        }
        let resolved = AdapterEvent::PermissionResolved {
            turn_id: self.turn_identity()?,
            external_request_id: super::identity::external("piPermissionId", request_id)?,
            state: ServerRequestState::Cancelled,
        };
        AdapterBatch::new(
            vec![resolved],
            vec![reply(
                id,
                Refusal::Withdrawn.message(),
                EgressToken::protocol_notification(),
            )?],
        )
    }

    pub(in crate::acp::bindings::pi::runtime) fn bridge_permission(
        &mut self,
        request_id: &SageServerRequestId,
        decision: &PermissionDecision,
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
        let allowed = *decision == PermissionDecision::AllowOnce;
        pending.approved = allowed;
        self.translator.take_pending_ui();
        let response = reply(
            request_id.as_str(),
            if allowed {
                "approved"
            } else {
                Refusal::Denied.message()
            },
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
                // Pi runs an approved command without a commit, so the
                // approval reaching Pi frees the Turn's single pending slot.
                if matches!(pending.prepared, files::Prepared::Shell(_)) {
                    bridge.pending = None;
                    return Ok(());
                }
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
