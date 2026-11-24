use serde_json::Value;

use super::{ClaudeSdkRuntime, MAX_OPEN_DELIVERIES, SdkBridge, sdk_frame};
use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterRuntimeError, EgressToken, HarnessAdapter},
        claude::sdk::bridge::UsageReply,
    },
    goal::NativeRpcReply,
};
use sage_acp_protocol::intent::CapabilitySnapshotV1;

impl ClaudeSdkRuntime {
    pub(super) fn prepare_goal(
        &mut self,
        action: &crate::acp::goal::GoalAction,
    ) -> Result<bool, AdapterRuntimeError> {
        use crate::acp::goal::{GoalAction, GoalCommand, valid_objective};
        let bridge = self.bridge.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        let capabilities = bridge.goal_capabilities();
        let input = match action {
            GoalAction::Set { objective }
                if capabilities.set
                    && valid_objective(objective)
                    && objective.trim() != "clear" =>
            {
                objective.as_str()
            }
            GoalAction::ClearExisting if capabilities.clear => "clear",
            _ => return Err(AdapterRuntimeError::UnsupportedCommand),
        };
        let commands = self.session.available_commands().snapshot();
        let name = bridge.goal_command_name();
        if !commands.commands.iter().any(|command| {
            command.name == name
                && command.source == crate::acp::slash_commands::CommandSource::Builtin
        }) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        self.native = Some(NativeRpcReply::GoalCommand(GoalCommand {
            revision: commands.revision,
            name: sage_acp_protocol::contract::RequiredText::new("goalCommand", name)
                .map_err(|_| AdapterRuntimeError::Operation)?,
            input: sage_acp_protocol::contract::RequiredText::new("goalInput", input)
                .map_err(|_| AdapterRuntimeError::Operation)?,
        }));
        Ok(true)
    }

    /// Only one native control request runs at a time, so its delivery token identifies it.
    pub(super) fn control_egress(
        &mut self,
        sdk: &Value,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sent = SdkBridge::sent(sdk).ok_or(AdapterRuntimeError::Frame)?;
        let frame = match sdk_frame(sdk) {
            Ok(frame) => frame,
            Err(error) => {
                self.bridge()?.undo(&sent);
                return Err(error);
            }
        };
        let token = EgressToken::protocol_notification();
        if self.sent.len() < MAX_OPEN_DELIVERIES {
            self.sent.push((token.clone(), sent));
        }
        Ok(AdapterEgress::new(frame, token))
    }

    pub(super) fn control_request(
        &mut self,
        control: &sage_acp_protocol::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let sage_acp_protocol::session_control::SessionControl::ForkAt { target } = control {
            let config = self.binding.config_directory();
            let sdk = self
                .bridge()?
                .fork_at(config, target.turn_id.clone())
                .map_err(|_| AdapterRuntimeError::Operation)?;
            return self.control_egress(&sdk);
        }
        let sdk = self
            .bridge()?
            .session_control(control)
            .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
        self.control_egress(&sdk)
    }

    pub(super) fn answered(&mut self, reply: Option<NativeRpcReply>, usage: Option<UsageReply>) {
        if let Some(mut reply) = reply {
            if let (NativeRpcReply::FileRewindPlan(files), Some(cwd)) = (&mut reply, &self.cwd) {
                for file in files.iter_mut().filter(|file| file.is_relative()) {
                    *file = cwd.join(&*file);
                }
            }
            self.native = Some(reply);
        }
        match usage {
            Some(UsageReply::Limits(snapshot)) => self.usage.replace(snapshot),
            Some(UsageReply::Unsupported) => self.usage.mark_unsupported(),
            None => {}
        }
    }

    pub(super) fn usage_request(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .usage_read()
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }

    pub(super) fn abandon_passive_requests(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.abandon_passive_requests();
        }
    }

    pub(super) fn fork_egress(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let config = self.binding.config_directory();
        let sdk = self
            .bridge()?
            .fork(config)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }
}

/// The session's capabilities and its `Initialized` event must agree.
pub(super) fn sdk_capabilities(
    mut capabilities: CapabilitySnapshotV1,
    version: &str,
) -> CapabilitySnapshotV1 {
    capabilities.active_turn_steer = capabilities.prompt;
    capabilities.native_session_compact = capabilities.prompt;
    capabilities.rewind = if capabilities.prompt {
        rewind_capability(version)
    } else {
        Default::default()
    };
    capabilities
}

/// The oldest Claude Code release seen to honour `dry_run` on
/// `rewind_files` (its control schema and `rewindFiles` client, read in the
/// 2.1.326 and 2.1.328 binaries). One that ignores the field would restore
/// files while Sage asked only for a preview.
const FILE_REWIND_PREVIEW_MINIMUM: &str = "2.1.326";

pub(crate) fn rewind_capability(version: &str) -> sage_acp_protocol::intent::RewindCapability {
    use crate::acp::bindings::release_version::ReleaseVersion;
    use sage_acp_protocol::intent::{RewindCapability, RewindEffects, RewindTargets};
    let file_preview = ReleaseVersion::parse(version)
        .zip(ReleaseVersion::parse(FILE_REWIND_PREVIEW_MINIMUM))
        .is_some_and(|(found, minimum)| found >= minimum);
    RewindCapability {
        effects: RewindEffects {
            conversation: true,
            files: true,
            file_preview,
        },
        targets: RewindTargets::AnyUserMessage,
        fork_at: true,
        on_resume: true,
        runtime_checkpoint: false,
        fork_latest: false,
    }
}
