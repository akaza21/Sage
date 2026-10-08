use super::*;

pub(crate) struct CompletedPumpParts<A, J, R> {
    pub(super) adapter: A,
    pub(super) journal: J,
    resources: R,
    publisher: Arc<dyn ProjectionPublisher>,
    session_intent: SessionPumpIntent,
    terminal_cursor: AdapterTurnCursor,
    next_synthetic_item: u64,
    pub(super) idle_decoder: IncrementalFrameDecoder,
}

type PumpHandoff<T, P> = Result<T, Box<(P, AdapterPumpError)>>;

impl<A: HarnessAdapter, J: AdapterBatchJournal, R: ResourceMapper> CompletedPumpParts<A, J, R> {
    pub(crate) async fn accept_idle_stdout_chunk(
        &mut self,
        chunk: &[u8],
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        if self.adapter.child_read_pending() {
            self.idle_decoder.enable_recoverable_oversize();
        } else {
            self.idle_decoder.disable_recoverable_oversize();
        }
        let mut frame = self
            .idle_decoder
            .push_chunk(chunk)
            .map_err(|_| AdapterPumpError::Frame)?;
        let mut nonfatal_refusal = None;
        loop {
            if let Some(id) = self.idle_decoder.take_oversize_id() {
                self.adapter
                    .accept_oversized_response(id)
                    .map_err(|_| AdapterPumpError::Frame)?;
                if !self.adapter.child_read_pending() {
                    self.idle_decoder.disable_recoverable_oversize();
                }
            }
            if let Some(current) = frame {
                validate_structure(&current).map_err(|_| AdapterPumpError::Frame)?;
                match self.adapter.ingest(&current, &self.resources) {
                    Ok(batch) => {
                        let (events, egress) = batch.into_parts();
                        if !egress.is_empty() {
                            return Err(AdapterPumpError::Frame);
                        }
                        for event in events {
                            match event {
                                AdapterEvent::ChildLiveItem {
                                    child_thread_id,
                                    child_turn_id,
                                    item_id,
                                    role,
                                    lifecycle,
                                    tool_state,
                                    payload,
                                } => {
                                    let measured = super::payload::payload_measure(&payload)?;
                                    let scope = TurnScope {
                                        project_id: self.session_intent.project_id.clone(),
                                        chat_id: self.session_intent.chat_id.clone(),
                                        session_id: self.session_intent.session_id.clone(),
                                        runtime_epoch: self.session_intent.runtime_epoch,
                                        turn_id: self.terminal_cursor.turn_id().clone(),
                                        binding_digest: hex(&self.session_intent.binding_digest),
                                    };
                                    self.journal
                                        .commit(
                                            vec![DurableAdapterEvent::ChildLiveItem {
                                                scope,
                                                child_thread_id,
                                                child_turn_id,
                                                item_id,
                                                role,
                                                lifecycle,
                                                tool_state,
                                                payload,
                                                payload_digest: measured.digest,
                                                now_millis,
                                            }],
                                            current.len().max(1),
                                        )
                                        .await
                                        .map_err(AdapterPumpError::Journal)?;
                                }
                                AdapterEvent::ChildTurnStatus {
                                    child_thread_id,
                                    child_turn_id,
                                    status,
                                } => {
                                    let scope = TurnScope {
                                        project_id: self.session_intent.project_id.clone(),
                                        chat_id: self.session_intent.chat_id.clone(),
                                        session_id: self.session_intent.session_id.clone(),
                                        runtime_epoch: self.session_intent.runtime_epoch,
                                        turn_id: self.terminal_cursor.turn_id().clone(),
                                        binding_digest: hex(&self.session_intent.binding_digest),
                                    };
                                    self.journal
                                        .commit(
                                            vec![DurableAdapterEvent::ChildTurnStatus {
                                                scope,
                                                child_thread_id,
                                                child_turn_id,
                                                status,
                                                now_millis,
                                            }],
                                            current.len().max(1),
                                        )
                                        .await
                                        .map_err(AdapterPumpError::Journal)?;
                                }
                                AdapterEvent::IgnoredLate
                                | AdapterEvent::IgnoredDuplicate
                                | AdapterEvent::Absorbed => {}
                                _ => return Err(AdapterPumpError::Frame),
                            }
                        }
                    }
                    Err(error) if let Some(refusal) = NonfatalRefusal::from_adapter(&error) => {
                        nonfatal_refusal = Some(refusal.into());
                    }
                    Err(error) => return Err(AdapterPumpError::Adapter(error)),
                }
            }
            if !self.idle_decoder.has_pending_input() {
                break;
            }
            frame = self
                .idle_decoder
                .drain_pending()
                .map_err(|_| AdapterPumpError::Frame)?;
        }
        nonfatal_refusal.map_or(Ok(()), Err)
    }
    pub(crate) fn idle_frame_aligned(&self) -> bool {
        self.idle_decoder.is_frame_aligned()
    }
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
        if !self.idle_decoder.is_frame_aligned() || self.adapter.config_pending() {
            return Err(Box::new((self, AdapterPumpError::Intent)));
        }
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
                        idle_decoder: self.idle_decoder,
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
            && self.pending_permissions.is_empty()
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
            idle_decoder: self.decoder,
        })
    }
}
