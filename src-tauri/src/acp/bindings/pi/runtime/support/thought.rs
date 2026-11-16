use super::super::PiRuntime;
use super::stream::shown_snapshot;
use crate::acp::bindings::adapter::{AdapterEvent, AdapterRuntimeError};
use crate::acp::bindings::pi::TerminalOutcome;
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload, lifecycle::ItemLifecycle};

impl PiRuntime {
    pub(super) fn start_thought(&mut self) -> Result<AdapterEvent, AdapterRuntimeError> {
        let id = self.next_local_item("thought")?;
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        if turn.thought_id.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        turn.thought_id = Some(id.clone());
        turn.thought_snapshot = Some(String::new());
        self.item(
            id,
            ItemLifecycle::Started,
            None,
            NormalizedPayload::ThoughtSummary {
                summary: SafeText::new("thoughtSummary", "")
                    .map_err(|_| AdapterRuntimeError::Frame)?,
            },
        )
    }

    pub(super) fn complete_thought(
        &mut self,
        outcome: TerminalOutcome,
    ) -> Result<Option<AdapterEvent>, AdapterRuntimeError> {
        let turn = self
            .active_turn
            .as_mut()
            .ok_or(AdapterRuntimeError::Operation)?;
        let Some(id) = turn.thought_id.take() else {
            return Ok(None);
        };
        let summary = shown_snapshot(
            &turn
                .thought_snapshot
                .take()
                .ok_or(AdapterRuntimeError::Operation)?,
        )?;
        let lifecycle = match outcome {
            TerminalOutcome::Completed => ItemLifecycle::Completed,
            TerminalOutcome::Cancelled | TerminalOutcome::Interrupted => ItemLifecycle::Cancelled,
            TerminalOutcome::Failed => ItemLifecycle::Failed,
        };
        Ok(Some(
            self.item(
                id,
                lifecycle,
                None,
                NormalizedPayload::ThoughtSummary {
                    summary: SafeText::new("thoughtSummary", summary)
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                },
            )?,
        ))
    }
}
