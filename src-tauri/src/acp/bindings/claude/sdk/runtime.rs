//! The launchable Claude Code runtime over the SDK protocol.
//!
//! Claude's session state machine (permissions, config, turns, auth) runs
//! unchanged on ACP frames; this runtime owns the CLI launch and passes every
//! frame through the bridge in both directions.

use std::sync::Arc;

use serde_json::Value;

use super::{
    admission::{AdmittedClaudeSdkBinding, SessionLaunch},
    bridge::{SdkBridge, Sent},
};
use crate::acp::{
    bindings::{
        adapter::{
            AdapterBatch, AdapterBatchBuilder, AdapterCommand, AdapterEgress, AdapterEvent,
            AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, RefusalDetail,
            ResourceMapper,
        },
        claude::{
            runtime::{Children, ClaudeRuntime},
            wire::decode_frame,
        },
        runtime::{LaunchableRuntime, NativeRuntime, RuntimeFactory},
    },
    contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::CapabilitySnapshotV1,
    process::LaunchSpec,
    runtime::ActivationMode,
    transport::{Frame, OutboundFrame, request_value_digest},
};

pub(crate) struct ClaudeSdkRuntimeFactory {
    pub(crate) binding: Arc<AdmittedClaudeSdkBinding>,
    pub(crate) root: crate::project_registry::ProjectRootAuthority,
}

impl RuntimeFactory for ClaudeSdkRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        self.binding
            .revalidate()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok(NativeRuntime::new(ClaudeSdkRuntime {
            session: ClaudeRuntime::for_session(
                self.binding.binding_digest(),
                runtime_epoch,
                session_id,
            ),
            bridge: None,
            children: Children::default(),
            native: None,
            sent: Vec::new(),
            steering: Steering::default(),
            launch: None,
            binding: self.binding,
            root: Some(self.root),
            epoch: runtime_epoch,
        }))
    }
}

struct ClaudeSdkRuntime {
    session: ClaudeRuntime,
    bridge: Option<SdkBridge>,
    children: Children,
    /// The reply to a session control, until the host takes it.
    native: Option<crate::acp::goal::NativeRpcReply>,
    /// Written frames whose delivery is still open, with what they started.
    sent: Vec<(EgressToken, Sent)>,
    steering: Steering,
    launch: Option<SessionLaunch>,
    binding: Arc<AdmittedClaudeSdkBinding>,
    root: Option<crate::project_registry::ProjectRootAuthority>,
    epoch: RuntimeEpoch,
}

impl ClaudeSdkRuntime {
    fn bridge(&mut self) -> Result<&mut SdkBridge, AdapterRuntimeError> {
        self.bridge.as_mut().ok_or(AdapterRuntimeError::Operation)
    }

    /// Rewrites one ACP egress as its SDK frame, keeping its delivery token.
    /// A frame Claude Code has no equivalent for is refused as never
    /// queued, so the session rolls back what the egress began.
    fn outbound(&mut self, egress: AdapterEgress) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = serde_json::from_slice::<Value>(egress.frame().payload())
            .map_err(|_| AdapterRuntimeError::Frame)
            .and_then(|acp| {
                self.bridge()?
                    .outbound(&acp)
                    .map_err(|_| AdapterRuntimeError::UnsupportedCommand)
            });
        let token = egress.into_token();
        let sent = sdk.as_ref().ok().and_then(SdkBridge::sent);
        // The SDK frame can outgrow the ACP frame it replaces (attachment
        // wrappers), so it is built before anything is recorded as sent.
        let frame = match sdk.and_then(|sdk| sdk_frame(&sdk)) {
            Ok(frame) => frame,
            Err(error) => {
                if let (Some(sent), Some(bridge)) = (&sent, self.bridge.as_mut()) {
                    bridge.undo(sent);
                }
                self.session
                    .resolve_egress(token, EgressResolution::QueueRejected)?;
                return Err(error);
            }
        };
        if let Some(sent) = sent {
            if self.sent.len() < MAX_OPEN_DELIVERIES {
                self.sent.push((token.clone(), sent));
            } else if let Some(bridge) = self.bridge.as_mut() {
                bridge.delivered(&sent);
            }
        }
        Ok(AdapterEgress::new(frame, token))
    }

    /// One SDK frame through the bridge, its ACP frames through the session.
    fn ingest_sdk(
        &mut self,
        sdk: Value,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let inbound = self
            .bridge()?
            .inbound(sdk)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let mut batch = AdapterBatchBuilder::new();
        let mut observed = false;
        for wire in inbound.withdrawn {
            let (events, _) = self.session.withdraw_permission(&wire.into())?.into_parts();
            for event in events
                .into_iter()
                .filter(|event| !matches!(event, AdapterEvent::Absorbed))
            {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for event in self.steering.answers(inbound.steers) {
            observed = true;
            batch.push_event(event)?;
        }
        if let Some(reply) = inbound.native {
            self.native = Some(reply);
        }
        if let Some(questions) = inbound.questions {
            self.session.offer_user_input(questions);
        }
        for event in inbound.session {
            if let Some(event) = self.session.session_event(event) {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for child in inbound.children {
            for event in self
                .session
                .child_event(&mut self.children, child, resources)?
            {
                observed = true;
                batch.push_event(event)?;
            }
        }
        for acp in inbound.acp {
            let digest = request_value_digest(&acp);
            let (events, egress) = self
                .session
                .ingest_value(acp, digest, resources)?
                .into_parts();
            for event in events {
                observed = true;
                batch.push_event(event)?;
            }
            for egress in egress {
                batch.push_egress(self.outbound(egress)?)?;
            }
        }
        for reply in inbound.replies {
            batch.push_egress(AdapterEgress::new(
                sdk_frame(&reply)?,
                EgressToken::protocol_notification(),
            ))?;
        }
        if !observed {
            batch.push_event(AdapterEvent::Absorbed)?;
        }
        Ok(batch.finish())
    }
}

/// Paths in a frame read without its structure checks are not mapped into
/// the project; they show as unavailable.
struct NoResources;

impl ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        Err(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
}

/// Deliveries the pump settles before more egress; beyond this, undelivered
/// frames are not undone (the pump fails the session on such writes anyway).
const MAX_OPEN_DELIVERIES: usize = 64;

fn sdk_frame(value: &Value) -> Result<OutboundFrame, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(value).map_err(|_| AdapterRuntimeError::Frame)?;
    // A user message carries the prompt's images and files, so it takes the
    // prompt frame bound, as the ACP `session/prompt` it came from did.
    if value.get("type").and_then(Value::as_str) == Some("user") {
        OutboundFrame::new_prompt(bytes)
    } else {
        OutboundFrame::new(bytes)
    }
    .map_err(|_| AdapterRuntimeError::Frame)
}

impl LaunchableRuntime for ClaudeSdkRuntime {
    fn bind_activation(&mut self, mode: &ActivationMode) {
        let (session_id, resume, model) = match mode {
            ActivationMode::New { model, .. } => (uuid::Uuid::new_v4().to_string(), false, model),
            ActivationMode::Resume {
                external_session_id,
                model,
                ..
            } => (external_session_id.as_str().to_owned(), true, model),
            ActivationMode::Load { .. } => return,
        };
        let model = model.as_ref().map(|model| model.as_str().to_owned());
        self.bridge = Some(SdkBridge::new(session_id.clone(), model.clone()));
        self.launch = Some(SessionLaunch {
            session_id,
            resume,
            model,
        });
    }

    fn launch_spec(&mut self) -> Result<LaunchSpec, AdapterRuntimeError> {
        let (Some(root), Some(launch)) = (self.root.take(), self.launch.as_ref()) else {
            return Err(AdapterRuntimeError::LaunchUnavailable);
        };
        self.binding.launch_spec(self.epoch.get(), root, launch)
    }
}

impl HarnessAdapter for ClaudeSdkRuntime {
    fn available_commands(&self) -> Arc<crate::acp::slash_commands::CommandState> {
        self.session.available_commands()
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.session.session_config()
    }
    fn refusal_detail(&self) -> Option<RefusalDetail> {
        self.session.refusal_detail()
    }
    fn config_pending(&self) -> bool {
        self.session.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.session
            .configure(intent)?
            .map(|egress| self.outbound(egress))
            .transpose()
    }
    /// Claude Code queues a message sent while it works (see
    /// `bridge::steer`), and compacts on its own `/compact` prompt, which a
    /// Compact action sends as its Turn.
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.session.capabilities().map(|mut capabilities| {
            capabilities.active_turn_steer = capabilities.prompt;
            capabilities.native_session_compact = capabilities.prompt;
            capabilities
        })
    }
    fn session_control(
        &mut self,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .session_control(control)
            .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
        let sent = SdkBridge::sent(&sdk).ok_or(AdapterRuntimeError::Frame)?;
        let frame = match sdk_frame(&sdk) {
            Ok(frame) => frame,
            Err(error) => {
                self.bridge()?.undo(&sent);
                return Err(error);
            }
        };
        // One control at a time, so its token finds it if the write fails.
        let token = EgressToken::protocol_notification();
        if self.sent.len() < MAX_OPEN_DELIVERIES {
            self.sent.push((token.clone(), sent));
        }
        Ok(AdapterEgress::new(frame, token))
    }
    fn native_rpc_pending(&self) -> bool {
        self.native.is_some()
            || self.bridge.as_ref().is_some_and(SdkBridge::controlling)
            || self.session.native_rpc_pending()
    }
    fn take_native_rpc_reply(&mut self) -> Option<crate::acp::goal::NativeRpcReply> {
        self.native
            .take()
            .or_else(|| self.session.take_native_rpc_reply())
    }
    /// A large tool result or file write is dropped rather than ending the
    /// session; its tool row ends as failed when the turn completes.
    fn skips_oversize_frames(&self) -> bool {
        true
    }

    /// The launch already names the session and its model, so the session
    /// commands open it without repeating the model.
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        let command = match command {
            AdapterCommand::NewSession { native_root, .. } => AdapterCommand::NewSession {
                native_root,
                model: None,
            },
            AdapterCommand::ResumeSession {
                external_session_id,
                native_root,
                ..
            } => AdapterCommand::ResumeSession {
                external_session_id,
                native_root,
                model: None,
            },
            AdapterCommand::Steer { prompt, .. } => {
                let bridge = self.bridge.as_mut().ok_or(AdapterRuntimeError::Operation)?;
                return self.steering.egress(bridge, &prompt);
            }
            command => command,
        };
        let egress = self.session.command(command)?;
        self.outbound(egress)
    }

    fn cancel_permission(
        &mut self,
        request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let egress = self.session.cancel_permission(request_id)?;
        self.outbound(egress)
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let sdk = decode_frame(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        self.ingest_sdk(sdk, resources)
    }

    /// Claude Code's own output can exceed the structure limits sized for
    /// ACP frames: an edit's result carries its whole patch in
    /// `tool_use_result`, which Sage never reads. Such a frame is read
    /// without those fields. Still out of bounds, a turn's end or a reply to
    /// Sage keeps only what ends it, a control request is refused so Claude
    /// Code is not left waiting, and anything else is dropped (an unfinished
    /// tool row ends as failed with the turn).
    fn discard_invalid_response(
        &mut self,
        frame: &Frame,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut sdk: Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| AdapterRuntimeError::Frame)?;
        let object = sdk.as_object_mut().ok_or(AdapterRuntimeError::Frame)?;
        object.remove("tool_use_result");
        object.remove("tool_result_meta");
        let reduced = serde_json::to_vec(&sdk).map_err(|_| AdapterRuntimeError::Frame)?;
        if let Ok(sdk) = decode_frame(&reduced) {
            return self.ingest_sdk(sdk, &NoResources);
        }
        if let Some(essentials) = SdkBridge::essentials(&sdk)
            && let Ok(essentials) = serde_json::to_vec(&essentials)
            && let Ok(sdk) = decode_frame(&essentials)
        {
            return self.ingest_sdk(sdk, &NoResources);
        }
        let mut batch = AdapterBatchBuilder::new();
        if let Some(reply) = SdkBridge::refusal(&sdk) {
            batch.push_egress(AdapterEgress::new(
                sdk_frame(&reply)?,
                EgressToken::protocol_notification(),
            ))?;
        }
        batch.push_event(AdapterEvent::Absorbed)?;
        Ok(batch.finish())
    }

    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.session.commit_mandatory_egress(token)
    }

    /// A frame Claude Code never received undoes what it started in the
    /// bridge too, so the next prompt or setting is not refused as busy.
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        if self
            .steering
            .resolve(&token, resolution, self.bridge.as_mut())
        {
            return Ok(());
        }
        if !matches!(resolution, EgressResolution::Queued)
            && let Some(index) = self.sent.iter().position(|(sent, _)| *sent == token)
        {
            let (_, sent) = self.sent.swap_remove(index);
            if let Some(bridge) = self.bridge.as_mut() {
                if matches!(
                    resolution,
                    EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
                ) {
                    bridge.undo(&sent);
                } else {
                    bridge.delivered(&sent);
                }
            }
        }
        self.session.resolve_egress(token, resolution)
    }
}

#[path = "runtime_steer.rs"]
mod steering;
use steering::Steering;

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
