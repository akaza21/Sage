use super::*;

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
    /// stdout that followed the frame starting it.
    pub(crate) async fn decline_started_turn(
        &mut self,
        now_millis: i64,
        transport: &mut impl crate::acp::runtime::adapter_pump::EgressTransport,
    ) -> Result<(), AdapterPumpError> {
        if !self.turn_started {
            return Err(AdapterPumpError::Intent);
        }
        let egress = self.adapter.decline_turn()?;
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
