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
        claude::{runtime::ClaudeRuntime, wire::decode_frame},
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
            sent: Vec::new(),
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
    /// Written frames whose delivery is still open, with what they started.
    sent: Vec<(EgressToken, Sent)>,
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
        let sdk = match sdk {
            Ok(sdk) => sdk,
            Err(error) => {
                self.session
                    .resolve_egress(token, EgressResolution::QueueRejected)?;
                return Err(error);
            }
        };
        if let Some(sent) = SdkBridge::sent(&sdk)
            && self.sent.len() < MAX_OPEN_DELIVERIES
        {
            self.sent.push((token.clone(), sent));
        }
        Ok(AdapterEgress::new(sdk_frame(&sdk)?, token))
    }
}

/// Deliveries the pump settles before more egress; beyond this, undelivered
/// frames are not undone (the pump fails the session on such writes anyway).
const MAX_OPEN_DELIVERIES: usize = 64;

fn sdk_frame(value: &Value) -> Result<OutboundFrame, AdapterRuntimeError> {
    let bytes = serde_json::to_vec(value).map_err(|_| AdapterRuntimeError::Frame)?;
    OutboundFrame::new(bytes).map_err(|_| AdapterRuntimeError::Frame)
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
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.session.capabilities()
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
        let inbound = self
            .bridge()?
            .inbound(sdk)
            .map_err(|_| AdapterRuntimeError::Frame)?;
        let mut batch = AdapterBatchBuilder::new();
        let mut observed = false;
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
        if !matches!(resolution, EgressResolution::Queued)
            && let Some(index) = self.sent.iter().position(|(sent, _)| *sent == token)
        {
            let (_, sent) = self.sent.swap_remove(index);
            if matches!(
                resolution,
                EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
            ) && let Some(bridge) = self.bridge.as_mut()
            {
                bridge.undo(&sent);
            }
        }
        self.session.resolve_egress(token, resolution)
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
