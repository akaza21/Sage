use super::*;
use crate::acp::{bindings::adapter::AdapterEvent, command_placement::NativeControl};

/// The live command catalog: its WebView channel, the revision a picked command
/// must match, and the events that ask the binding to re-read it.
impl NativeRuntime {
    pub(crate) fn attach_commands(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        channel: std::sync::Arc<crate::acp::slash_commands::CommandChannel>,
    ) {
        self.3 = Some((owner, channel));
    }

    pub(super) fn publish_commands(&self) {
        if let Some((owner, channel)) = &self.3 {
            channel.update(owner, self.0.available_commands());
        }
    }

    pub(super) fn discover_catalog(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let commands = self.0.available_commands();
        let result = if commands.take_refresh_request() {
            let result = self.0.refresh_commands();
            // An unsent refresh stays asked for until the next discovery: a
            // read already out may predate the change. A harness that cannot
            // refresh never will.
            if !matches!(
                result,
                Ok(Some(_)) | Err(AdapterRuntimeError::UnsupportedCommand)
            ) {
                commands.request_refresh();
            }
            result
        } else {
            self.0.discover_commands()
        };
        if let Some((owner, channel)) = &self.3 {
            channel.update(owner, commands);
        }
        result
    }

    /// Sends a prompt only when its picked command belongs to the current
    /// catalog revision, and re-reads the catalog after a reload command.
    pub(super) fn command_with_refresh(
        &mut self,
        command: AdapterCommand,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let commands = self.0.available_commands();
        if let AdapterCommand::Prompt { prompt, .. } | AdapterCommand::Steer { prompt, .. } =
            &command
            && let Some(revision) = prompt.command_revision
        {
            commands.validate(revision, prompt.as_str())?;
        }
        let reload = match &command {
            AdapterCommand::Prompt { prompt, .. } | AdapterCommand::Steer { prompt, .. } => prompt
                .command_revision
                .and_then(|revision| commands.command(revision, prompt.as_str()).ok())
                .is_some_and(|command| {
                    command.placement.control == Some(NativeControl::ReloadAddOns)
                }),
            _ => false,
        };
        let egress = self.0.command(command)?;
        if reload {
            commands.refresh_after_turn();
        }
        Ok(egress)
    }

    pub(super) fn refresh_after_fork(&self, reply: Option<&crate::acp::goal::NativeRpcReply>) {
        if matches!(reply, Some(crate::acp::goal::NativeRpcReply::Forked(_))) {
            self.0.available_commands().request_refresh();
        }
    }

    pub(super) fn follow_catalog_triggers(&self, batch: &AdapterBatch) {
        let commands = self.0.available_commands();
        for event in batch.events() {
            if let AdapterEvent::Turn { state, .. } = event
                && state.is_terminal()
            {
                commands.turn_completed();
            }
        }
    }
}
