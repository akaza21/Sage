use super::super::PiRuntime;
use super::stream::closed_snapshot;
use crate::acp::bindings::adapter::{AdapterEvent, AdapterRuntimeError};
use crate::acp::bindings::pi::TerminalOutcome;
use crate::acp::redaction::text::ReplyStream;
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload, lifecycle::ItemLifecycle};

impl PiRuntime {
    /// The reply and thought still streaming when Pi's output ended, shown
    /// whole for the turn's sweep to settle.
    pub(in crate::acp::bindings::pi) fn whole_unfinished_replies(
        &mut self,
    ) -> Result<Vec<AdapterEvent>, AdapterRuntimeError> {
        let Some(turn) = self.active_turn.as_mut() else {
            return Ok(Vec::new());
        };
        let mut replies = Vec::new();
        if let Some(id) = turn.text_id.take()
            && turn.text_snapshot.is_some()
        {
            let text = closed_snapshot(&mut turn.text_snapshot)?;
            replies.push((id, super::identity::text_payload(text)));
        }
        if let Some(id) = turn.thought_id.take()
            && turn.thought_snapshot.is_some()
        {
            let summary = SafeText::new(
                "thoughtSummary",
                closed_snapshot(&mut turn.thought_snapshot)?,
            )
            .map_err(|_| AdapterRuntimeError::Frame)?;
            replies.push((id, NormalizedPayload::ThoughtSummary { summary }));
        }
        replies
            .into_iter()
            .map(|(id, payload)| self.item(id, ItemLifecycle::Streaming, None, payload))
            .collect()
    }

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
        turn.thought_snapshot = Some(ReplyStream::default());
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
        let summary = closed_snapshot(&mut turn.thought_snapshot)?;
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
