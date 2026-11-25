use super::*;
use sage_acp_protocol::intent::session_event::{
    PendingSessionEvents, SessionNotice, SessionNoticeLevel, SessionScopedEvent,
};

const REFUSED_TURN: &str = "The agent started a turn this chat cannot run with its current access or state, so the session was stopped. Send a message to continue.";

/// Why the session stopped when a harness keeps running a Turn Sage refused.
pub(crate) fn refused_turn_notice() -> Result<SessionNotice, AdapterPumpError> {
    Ok(SessionNotice {
        identity: None,
        level: SessionNoticeLevel::Error,
        diagnostic: false,
        text: RequiredText::new("text", REFUSED_TURN).map_err(|_| AdapterPumpError::Frame)?,
        source: None,
    })
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(crate) fn turn_started(&self) -> bool {
        self.turn_started
    }

    pub(crate) fn harness_turn_authority(
        &self,
    ) -> Option<crate::acp::bindings::adapter::HarnessTurnAuthority> {
        self.adapter.harness_turn_authority()
    }

    /// Refuses the Turn the harness started, then reads on through the
    /// stdout that followed the frame starting it. A harness that cannot
    /// decline it keeps running it, so the session must stop: the chat is
    /// told why first, before any later frame is read.
    pub(crate) async fn decline_started_turn(
        &mut self,
        now_millis: i64,
        transport: &mut impl crate::acp::runtime::adapter_pump::EgressTransport,
    ) -> Result<(), AdapterPumpError> {
        if !self.turn_started {
            return Err(AdapterPumpError::Intent);
        }
        let egress = match self.adapter.decline_turn() {
            Ok(egress) => egress,
            Err(error) => {
                let mut notice = PendingSessionEvents::default();
                notice.push(SessionScopedEvent::Notice(refused_turn_notice()?));
                self.commit_session_events(notice, REFUSED_TURN.len(), now_millis)
                    .await?;
                return Err(error.into());
            }
        };
        self.turn_started = false;
        crate::acp::runtime::adapter_pump::write_idle_egress(&mut self.adapter, egress, transport)?;
        let frame = if self.idle_decoder.has_pending_input() {
            self.idle_decoder
                .drain_pending()
                .map_err(|_| AdapterPumpError::Frame)?
        } else {
            None
        };
        self.read_idle_frames(frame, 0, now_millis, transport).await
    }

    /// Binds the Turn the harness started. The stdout that followed the
    /// frame starting it stays in the decoder for that Turn to read.
    pub(crate) fn bind_harness_turn(
        self,
        active_turn: ActiveTurnIntent,
    ) -> PumpHandoff<AdapterPump<A, J, R>, Self> {
        if !self.turn_started
            || (self.adapter.config_pending()
                && self.adapter.harness_turn_authority()
                    != Some(crate::acp::bindings::adapter::HarnessTurnAuthority::Questions))
        {
            return Err(Box::new((self, AdapterPumpError::Intent)));
        }
        self.rebind(active_turn)
    }
}
