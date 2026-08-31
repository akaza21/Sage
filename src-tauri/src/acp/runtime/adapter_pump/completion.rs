use super::*;

pub(crate) struct CompletedPumpParts<A, J, R> {
    pub(super) adapter: A,
    pub(super) journal: J,
    resources: R,
    publisher: Arc<dyn ProjectionPublisher>,
    session_intent: SessionPumpIntent,
    terminal_cursor: AdapterTurnCursor,
    next_synthetic_item: u64,
}

type PumpHandoff<T, P> = Result<T, Box<(P, AdapterPumpError)>>;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(crate) fn terminal_cursor(&self) -> &AdapterTurnCursor {
        &self.terminal_cursor
    }

    pub(crate) fn write_close_session(&mut self, transport: &mut impl super::EgressTransport) {
        let Ok(egress) = self.adapter.command(AdapterCommand::CloseSession) else {
            return;
        };
        let resolution = match transport.write(egress.frame()) {
            FrameWriteOutcome::Complete => EgressResolution::Written,
            FrameWriteOutcome::ZeroByteFailure => EgressResolution::WriteZeroBytes,
            FrameWriteOutcome::DeliveryUnknown { .. } => EgressResolution::DeliveryUnknown,
        };
        let _ = self.adapter.resolve_egress(egress.into_token(), resolution);
    }

    pub(crate) fn bind_turn(
        self,
        active_turn: ActiveTurnIntent,
    ) -> PumpHandoff<AdapterPump<A, J, R>, Self> {
        let intent = self.session_intent.clone().bind_turn(active_turn);
        let admission = match AdapterPump::<A, J, R>::admit(&intent) {
            Ok(admission) => admission,
            Err(error) => return Err(Box::new((self, error))),
        };
        let journal = match self.journal.try_rebind(intent.clone()) {
            Ok(journal) => journal,
            Err((journal, error)) => {
                return Err(Box::new((
                    Self {
                        journal,
                        adapter: self.adapter,
                        resources: self.resources,
                        publisher: self.publisher,
                        session_intent: self.session_intent,
                        terminal_cursor: self.terminal_cursor,
                        next_synthetic_item: self.next_synthetic_item,
                    },
                    AdapterPumpError::Journal(error),
                )));
            }
        };
        let next_synthetic_item = self.next_synthetic_item;
        let mut pump = AdapterPump::from_admission(
            self.adapter,
            journal,
            self.resources,
            self.publisher,
            intent,
            admission,
        );
        pump.next_synthetic_item = next_synthetic_item;
        Ok(pump)
    }
}

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> AdapterPump<A, J, R> {
    pub(crate) fn completed_turn_ready(&self) -> bool {
        let data_empty = self.egress.data_usage().0 == 0;
        let control_empty = self.egress.control_usage().0 == 0;
        !self.teardown
            && !self.projection_recovery_required
            && self.terminal.is_some()
            && self.pending.is_empty()
            && self.decoder_frame_aligned
            && data_empty
            && control_empty
    }

    /// Hands the sequential session-owned resources back only after the exact
    /// Turn terminal is durable and every stream/queue boundary is quiescent.
    pub(crate) fn try_into_completed_parts(self) -> PumpHandoff<CompletedPumpParts<A, J, R>, Self> {
        if !self.completed_turn_ready() {
            return Err(Box::new((self, AdapterPumpError::TeardownRequired)));
        }
        let Some(terminal_cursor) = self.durable_turn_cursor.clone() else {
            return Err(Box::new((self, AdapterPumpError::TeardownRequired)));
        };
        let session_intent = SessionPumpIntent {
            binding_digest: self.intent.binding_digest,
            project_id: self.intent.project_id,
            chat_id: self.intent.chat_id,
            session_id: self.intent.session_id,
            external_session_id: self.intent.external_session_id,
            runtime_epoch: self.intent.runtime_epoch,
        };
        Ok(CompletedPumpParts {
            adapter: self.adapter,
            journal: self.journal,
            resources: self.resources,
            publisher: self.publisher,
            session_intent,
            terminal_cursor,
            next_synthetic_item: self.next_synthetic_item,
        })
    }
}
