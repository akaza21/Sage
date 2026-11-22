use super::*;
use crate::acp::{bindings::adapter::AdapterEvent, command_placement::NativeControl};

impl NativeRuntime {
    pub(super) fn discover_catalog(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let commands = self.0.available_commands();
        if commands.refresh_pending() {
            return Ok(None);
        }
        let result = if commands.refresh_requested() {
            self.0.refresh_commands()
        } else {
            self.0.discover_commands()
        };
        if result.is_err() {
            commands.finish_refresh();
        }
        if let Some((owner, channel)) = &self.3 {
            channel.update_catalog(owner, commands, self.0.capabilities());
        }
        result
    }

    pub(super) fn command_with_refresh(
        &mut self,
        command: AdapterCommand,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let commands = self.0.available_commands();
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

    pub(super) fn follow_catalog_triggers(&self, batch: &AdapterBatch) {
        let commands = self.0.available_commands();
        for event in batch.events() {
            match event {
                AdapterEvent::SessionReady { .. } => commands.request_refresh(),
                AdapterEvent::Turn { state, .. } if state.is_terminal() => {
                    commands.turn_completed()
                }
                _ => {}
            }
        }
    }
}
