//! Pi dialogs share one visible request and four waiting slots per chat.
//! Idle adoption grants only a question lifecycle, never tool authority.

use super::super::{PiRuntime, TurnIntent};
use crate::acp::bindings::{
    adapter::{AdapterBatch, AdapterEgress, AdapterEvent, AdapterRuntimeError},
    pi::{AdapterError, AdapterOutput, decode::Record, translator::PendingUiRequest},
};
use sage_acp_protocol::{contract::RequiredText, lifecycle::TurnState};

mod state;
pub(in crate::acp::bindings::pi::runtime) use state::Questions;

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn next_question_deadline(
        &self,
    ) -> Option<std::time::Instant> {
        self.questions
            .queued
            .iter()
            .filter_map(PendingUiRequest::deadline)
            .min()
    }

    pub(in crate::acp::bindings::pi::runtime) fn expire_queued_questions(
        &mut self,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        let mut replies = Vec::new();
        let mut index = 0;
        while index < self.questions.queued.len() {
            if self.questions.queued[index].remaining_ms(self.translator.question_now()) == Some(0)
            {
                let pending = self
                    .questions
                    .queued
                    .remove(index)
                    .ok_or(AdapterRuntimeError::Operation)?;
                replies.push(self.cancel_dialog_reply(pending.id)?);
            } else {
                index += 1;
            }
        }
        if replies.is_empty() {
            Ok(None)
        } else {
            AdapterBatch::new(Vec::new(), replies).map(Some)
        }
    }
    pub(super) fn ingest_question(
        &mut self,
        record: Record,
    ) -> Result<Vec<AdapterOutput>, AdapterError> {
        if self.questions.cancelling()
            && let Record::Event { value, .. } = &record
        {
            let id = crate::acp::bindings::pi::decode::external_id(value.get("id"))?
                .ok_or(AdapterError::Malformed)?;
            return Ok(vec![AdapterOutput::QuestionCancelled { external_id: id }]);
        }
        let previous = self.translator.take_pending_ui();
        let outputs = self.translator.ingest_record(record);
        let next = self.translator.take_pending_ui();
        if let Some(previous) = previous {
            self.translator.remember_pending_ui(previous);
        }
        if let Some(next) = next {
            let duplicate = self.translator.permission_waiting_for(&next.id)
                || self
                    .questions
                    .queued
                    .iter()
                    .any(|pending| pending.id == next.id);
            if duplicate {
                return Ok(vec![AdapterOutput::DuplicateIgnored]);
            }
            if self.translator.pending_ui().is_some() || !self.questions.queued.is_empty() {
                if self.questions.queued.len() == 4 {
                    return Ok(vec![AdapterOutput::DialogRefused {
                        external_id: next.id,
                    }]);
                }
                self.questions.queued.push_back(next);
                return Ok(vec![AdapterOutput::Absorbed]);
            }
            self.questions.reply_pending = false;
            self.translator.remember_pending_ui(next);
        }
        outputs
    }

    pub(super) fn question_arrived(
        &mut self,
        id: String,
        digest: [u8; 32],
    ) -> Result<AdapterEvent, AdapterRuntimeError> {
        if self.active_turn.is_some() {
            return self.permission_event(id, digest);
        }
        let session = self
            .external_session
            .clone()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.questions.start();
        Ok(AdapterEvent::TurnStarted {
            id: self.identity(session, None, None),
        })
    }

    pub(in crate::acp::bindings::pi::runtime) fn adopt_questions(
        &mut self,
        client: RequiredText<512>,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if !self.questions.starting() || self.active_turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        self.questions.adopt();
        let pending = self
            .translator
            .pending_ui()
            .ok_or(AdapterRuntimeError::Operation)?;
        let (id, digest) = (pending.id.clone(), pending.digest);
        self.active_turn = Some(TurnIntent::new(
            super::identity::external("piQuestionTurn", client.as_str().to_owned())?,
            client,
        ));
        if self
            .translator
            .pending_ui()
            .is_some_and(|pending| pending.remaining_ms(self.translator.question_now()) == Some(0))
        {
            let reply = self.cancel_pending_question(&id)?;
            return AdapterBatch::new(Vec::new(), vec![reply]);
        }
        AdapterBatch::new(vec![self.permission_event(id, digest)?], Vec::new())
    }

    pub(in crate::acp::bindings::pi::runtime) fn advance_questions(
        &mut self,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        if !self.questions.reply_pending
            || self.questions.settling.is_some()
            || self.translator.pending_ui().is_some()
        {
            return Ok(None);
        }
        self.questions.reply_pending = false;
        self.deny = None;
        let mut events = Vec::new();
        let mut egress = Vec::new();
        while let Some(next) = self.questions.queued.pop_front() {
            if self.questions.cancelling() {
                egress.push(self.cancel_waiting_question(next.id)?);
                continue;
            }
            if next.remaining_ms(self.translator.question_now()) == Some(0) {
                egress.push(self.cancel_dialog_reply(next.id)?);
                continue;
            }
            let (id, digest) = (next.id.clone(), next.digest);
            self.translator.remember_pending_ui(next);
            events.extend(self.waiting_cancelled_notice()?);
            events.push(self.permission_event(id, digest)?);
            return AdapterBatch::new(events, egress).map(Some);
        }
        events.extend(self.waiting_cancelled_notice()?);
        if self.questions.idle_turn() {
            let cancelled = self.questions.cancelling();
            self.questions.finish();
            events.push(AdapterEvent::Turn {
                id: self.turn_identity()?,
                state: if cancelled {
                    TurnState::Cancelled
                } else {
                    TurnState::Completed
                },
            });
            self.active_turn = None;
        }
        if events.is_empty() && egress.is_empty() {
            Ok(None)
        } else {
            AdapterBatch::new(events, egress).map(Some)
        }
    }

    pub(in crate::acp::bindings::pi::runtime) fn cancel_question_turn(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if !self.questions.idle_turn() {
            return Ok(None);
        }
        let pending = self
            .translator
            .take_pending_ui()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.questions.visible = None;
        self.questions.cancel();
        self.questions.reply_pending = true;
        self.questions.settling = Some(pending.id.clone());
        self.cancel_dialog_reply(pending.id).map(Some)
    }

    pub(in crate::acp::bindings::pi::runtime) fn cancel_pending_question(
        &mut self,
        id: &str,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self.translator.permission_waiting_for(id) {
            return Err(AdapterRuntimeError::LaterPermissionMiss);
        }
        self.translator.take_pending_ui();
        self.discard_bridge_edit();
        self.deny = None;
        self.questions.visible = None;
        self.questions.cancel();
        self.questions.reply_pending = true;
        self.questions.settling = Some(id.to_owned());
        self.cancel_dialog_reply(id.to_owned())
    }

    pub(super) fn finish_questions(&mut self) {
        let remaining = usize::from(self.questions.visible.is_some()) + self.questions.queued.len();
        if self.questions.cancelled.len() + remaining > 5 {
            self.poison();
            return;
        }
        if let Some(id) = self.questions.visible.take() {
            self.questions.cancelled.push_back(id);
        }
        while let Some(pending) = self.questions.queued.pop_front() {
            self.questions.cancelled.push_back(pending.id);
        }
        self.questions.reply_pending = false;
        self.questions.finish();
    }

    pub(in crate::acp::bindings::pi::runtime) fn question_reply_written(&mut self, id: &str) {
        if self.questions.settling.as_deref() != Some(id) {
            return;
        }
        let id = self.questions.settling.take().expect("matched question");
        if self.recent_cancelled_dialogs.len()
            == sage_core::limits::LIMITS.queues.outstanding_rpc_per_session as usize
        {
            self.recent_cancelled_dialogs.pop_front();
        }
        self.recent_cancelled_dialogs.push_back(id);
    }

    pub(in crate::acp::bindings::pi::runtime) fn cancelled_questions(
        &mut self,
    ) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        // A stop reports its count once the queue drains; only a count left
        // after the stop's turn has ended is flushed here.
        let late_count = !self.questions.cancelling() && self.questions.waiting_cancelled > 0;
        if self.questions.cancelled.is_empty() && !late_count {
            return Ok(None);
        }
        let mut events = Vec::new();
        if late_count {
            events.extend(self.waiting_cancelled_notice()?);
        }
        let mut egress = Vec::new();
        while let Some(id) = self.questions.cancelled.pop_front() {
            let (event, reply) = self.cancel_dialog(id)?;
            events.extend(event);
            egress.extend(reply);
        }
        AdapterBatch::new(events, egress).map(Some)
    }

    pub(super) fn refuse_dialog(
        &mut self,
        id: String,
    ) -> Result<(Option<AdapterEvent>, Option<AdapterEgress>), AdapterRuntimeError> {
        let reply = self.cancel_dialog_reply(id)?;
        let event = if self.questions.notices < 8 {
            self.questions.notices += 1;
            self.question_notice(
                "Pi dialog cancelled: at most 50 options of 256 bytes and four waiting questions are supported.".to_owned(),
            )?
        } else {
            AdapterEvent::Absorbed
        };
        Ok((Some(event), Some(reply)))
    }

    /// Cancels a supported waiting question without a transcript item; the
    /// stop reports all of them in one counted notice.
    pub(super) fn cancel_waiting_question(
        &mut self,
        id: String,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.questions.waiting_cancelled = self.questions.waiting_cancelled.saturating_add(1);
        self.cancel_dialog_reply(id)
    }

    fn waiting_cancelled_notice(&mut self) -> Result<Option<AdapterEvent>, AdapterRuntimeError> {
        let text = match std::mem::take(&mut self.questions.waiting_cancelled) {
            0 => return Ok(None),
            1 => "Pi: 1 waiting question cancelled.".to_owned(),
            count => format!("Pi: {count} waiting questions cancelled."),
        };
        self.question_notice(text).map(Some)
    }

    fn question_notice(&self, text: String) -> Result<AdapterEvent, AdapterRuntimeError> {
        use sage_acp_protocol::intent::session_event::{
            SessionNotice, SessionNoticeLevel, SessionScopedEvent,
        };
        Ok(
            self.session_event(SessionScopedEvent::Notice(SessionNotice {
                usage_limit: None,
                identity: None,
                diagnostic: false,
                level: SessionNoticeLevel::Warning,
                text: RequiredText::new("noticeText", text)
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                source: Some(
                    RequiredText::new("noticeSource", "Pi")
                        .map_err(|_| AdapterRuntimeError::Frame)?,
                ),
            })),
        )
    }

    pub(in crate::acp::bindings::pi::runtime) fn decline_questions(
        &mut self,
    ) -> Result<Vec<AdapterEgress>, AdapterRuntimeError> {
        if !self.questions.starting() {
            return Err(AdapterRuntimeError::Operation);
        }
        self.questions.finish();
        let mut replies = Vec::new();
        if let Some(pending) = self.translator.take_pending_ui() {
            replies.push(self.cancel_dialog_reply(pending.id)?);
        }
        while let Some(pending) = self.questions.queued.pop_front() {
            replies.push(self.cancel_dialog_reply(pending.id)?);
        }
        Ok(replies)
    }
}
