use std::time::{Duration, Instant};

use crate::acp::{scheduler::RuntimeOwner, store::AdapterTurnCursor};

use super::{
    mailbox::HostMailbox,
    types::{
        HostCancelIntent, HostCloseIntent, HostCommand, HostError, HostOutcome, HostPromptIntent,
        HostReply, HostState, PromptWrite, RuntimeEngine,
    },
};

pub(super) struct Actor<E> {
    engine: E,
    state: HostState,
    owner: Option<RuntimeOwner>,
    active_turn: Option<crate::acp::contract::SageTurnId>,
    pending_prompt: bool,
    pending_cancel: Option<PendingCancel>,
    pending_terminal: Option<PendingTerminal>,
    pending_close: Option<PendingClose>,
}

struct PendingTerminal {
    owner: RuntimeOwner,
    cursor: AdapterTurnCursor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CancelPhase {
    Begin,
    Dispatch,
}

struct PendingCancel {
    intent: HostCancelIntent,
    phase: CancelPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClosePhase {
    Begin,
    Finish,
}

struct PendingClose {
    intent: HostCloseIntent,
    phase: ClosePhase,
}

impl<E: RuntimeEngine> Actor<E> {
    pub(super) fn new(engine: E) -> Self {
        Self {
            engine,
            state: HostState::Cold,
            owner: None,
            active_turn: None,
            pending_prompt: false,
            pending_cancel: None,
            pending_terminal: None,
            pending_close: None,
        }
    }

    pub(super) fn run(mut self, mailbox: &HostMailbox) {
        while let Some(envelope) = mailbox.wait_next() {
            let stop = matches!(envelope.command, HostCommand::Shutdown);
            let result = self.dispatch(envelope.command, envelope.deadline);
            let stopped = stop && matches!(result, Ok(HostReply::Outcome(HostOutcome::Shutdown)));
            let _ = envelope.reply.send(result);
            if stopped {
                mailbox.close();
                return;
            }
        }
        // Engine ownership must never be leaked: dropping the process driver
        // retains the platform supervisor's bounded emergency teardown path.
        let _ = self.engine.shutdown(
            Instant::now()
                + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close),
        );
    }

    fn dispatch(
        &mut self,
        command: HostCommand,
        deadline: Instant,
    ) -> Result<HostReply, HostError> {
        match command {
            HostCommand::ResolvePermission {
                owner,
                request_id,
                decision,
            } => self.resolve_permission(owner, request_id, decision),
            HostCommand::Activate(owner) => self.activate(owner),
            HostCommand::PreparePrompt(intent) => self.prepare_prompt(intent),
            HostCommand::DispatchPrompt { owner, turn_id } => self.dispatch_prompt(owner, turn_id),
            HostCommand::Cancel(intent) => self.cancel(intent),
            HostCommand::Terminal { owner, cursor } => self.terminal(owner, cursor),
            HostCommand::ProjectSwitched(_) => {
                Ok(HostReply::Outcome(if self.state == HostState::Running {
                    HostOutcome::BackgroundTurnContinues
                } else {
                    HostOutcome::ProjectSwitchObserved
                }))
            }
            HostCommand::State => Ok(HostReply::State(self.state)),
            HostCommand::BeginClose(intent) => self.begin_close(intent),
            HostCommand::FinishClose(intent) => self.finish_close(intent),
            HostCommand::RecoverClose(intent) => self.recover_close(intent),
            HostCommand::Shutdown => self.shutdown(deadline),
        }
    }

    fn activate(&mut self, owner: RuntimeOwner) -> Result<HostReply, HostError> {
        if self.state != HostState::Cold
            || owner.runtime_epoch == crate::acp::contract::RuntimeEpoch::ZERO
        {
            return Err(HostError::State);
        }
        self.state = HostState::Activating;
        match self.engine.activate(&owner) {
            Ok(()) => {
                self.owner = Some(owner);
                self.state = HostState::Idle;
                Ok(HostReply::Outcome(HostOutcome::Activated))
            }
            Err(error) => {
                self.state = if matches!(
                    error,
                    HostError::Unavailable | HostError::Intent | HostError::Capacity
                ) {
                    HostState::Cold
                } else {
                    HostState::Recovery
                };
                Err(error)
            }
        }
    }

    fn resolve_permission(
        &mut self,
        owner: RuntimeOwner,
        request_id: crate::acp::contract::SageServerRequestId,
        decision: crate::acp::runtime::permissions::PermissionDecision,
    ) -> Result<HostReply, HostError> {
        if !matches!(self.state, HostState::Idle | HostState::Running)
            || !self.matches_owner(&owner)
        {
            return Err(HostError::Intent);
        }
        self.engine
            .resolve_permission(&owner, &request_id, decision)?;
        Ok(HostReply::Outcome(HostOutcome::PermissionResolved))
    }

    fn prepare_prompt(&mut self, intent: HostPromptIntent) -> Result<HostReply, HostError> {
        if self.state != HostState::Idle || !self.matches_owner(&intent.owner()) {
            return Err(HostError::Intent);
        }
        let turn_id = match self.engine.prepare_prompt(&intent) {
            Ok(turn_id) => turn_id,
            Err(error) => {
                self.state = if matches!(error, HostError::Capacity | HostError::Intent) {
                    HostState::Idle
                } else {
                    HostState::Recovery
                };
                return Err(error);
            }
        };
        self.active_turn = Some(turn_id.clone());
        self.pending_prompt = true;
        self.state = HostState::Running;
        Ok(HostReply::Prepared(turn_id))
    }

    fn dispatch_prompt(
        &mut self,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
    ) -> Result<HostReply, HostError> {
        if self.state != HostState::Running
            || !self.matches_owner(&owner)
            || self.active_turn.as_ref() != Some(&turn_id)
        {
            return Err(HostError::Intent);
        }
        if !self.pending_prompt {
            return Err(HostError::State);
        }
        let outcome = (|| -> Result<HostOutcome, HostError> {
            let first = self.engine.dispatch_prompt(&owner, &turn_id)?;
            match first {
                PromptWrite::Complete(returned) if returned == turn_id => {
                    Ok(HostOutcome::PromptAccepted)
                }
                PromptWrite::ZeroBytes(returned) if returned == turn_id => {
                    match self.engine.retry_prompt(&owner, &turn_id)? {
                        PromptWrite::Complete(retry_turn_id) if retry_turn_id == turn_id => {
                            Ok(HostOutcome::PromptAccepted)
                        }
                        PromptWrite::ZeroBytes(retry_turn_id) if retry_turn_id == turn_id => {
                            self.engine.record_retry_exhausted(&owner, &turn_id)?;
                            Err(HostError::RetryExhausted)
                        }
                        PromptWrite::PossibleBytes(retry_turn_id) if retry_turn_id == turn_id => {
                            self.engine.record_delivery_unknown(&owner, &turn_id)?;
                            Err(HostError::DeliveryUnknown)
                        }
                        _ => Err(HostError::Intent),
                    }
                }
                PromptWrite::PossibleBytes(returned) if returned == turn_id => {
                    self.engine.record_delivery_unknown(&owner, &turn_id)?;
                    Err(HostError::DeliveryUnknown)
                }
                _ => Err(HostError::Intent),
            }
        })();
        match outcome {
            Ok(outcome) => {
                self.pending_prompt = false;
                Ok(HostReply::Outcome(outcome))
            }
            Err(error) => {
                self.pending_prompt = false;
                self.state = HostState::Recovery;
                Err(error)
            }
        }
    }

    fn cancel(&mut self, intent: HostCancelIntent) -> Result<HostReply, HostError> {
        let retry_begin = self.state == HostState::Recovery
            && self.pending_cancel.as_ref().is_some_and(|pending| {
                pending.intent == intent && pending.phase == CancelPhase::Begin
            });
        if !retry_begin {
            if self.state == HostState::Recovery
                && self.pending_cancel.as_ref().is_some_and(|pending| {
                    pending.intent == intent && pending.phase == CancelPhase::Dispatch
                })
            {
                return Err(HostError::RecoveryRequired);
            }
            if self.state != HostState::Running
                || !self.matches_owner(&intent.owner)
                || self.active_turn.as_ref() != Some(&intent.turn_id)
                || self.pending_prompt
            {
                return Err(HostError::Intent);
            }
            self.pending_cancel = Some(PendingCancel {
                intent: intent.clone(),
                phase: CancelPhase::Begin,
            });
        }
        if let Err(error) = self.engine.begin_cancel(&intent) {
            if matches!(error, HostError::Capacity | HostError::Intent) {
                self.pending_cancel = None;
                self.state = HostState::Running;
            } else {
                self.state = HostState::Recovery;
            }
            return Err(error);
        }
        self.pending_cancel
            .as_mut()
            .expect("cancel intent retained")
            .phase = CancelPhase::Dispatch;
        if let Err(error) = self.engine.dispatch_cancel(&intent) {
            self.state = HostState::Recovery;
            return Err(error);
        }
        self.pending_cancel = None;
        self.state = HostState::Running;
        Ok(HostReply::Outcome(HostOutcome::CancelQueued))
    }

    fn terminal(
        &mut self,
        owner: RuntimeOwner,
        cursor: AdapterTurnCursor,
    ) -> Result<HostReply, HostError> {
        let retry = self.state == HostState::Recovery
            && self
                .pending_terminal
                .as_ref()
                .is_some_and(|pending| pending.owner == owner && pending.cursor == cursor);
        if !retry {
            if self.state != HostState::Running
                || !self.matches_owner(&owner)
                || self.active_turn.as_ref() != Some(cursor.turn_id())
                || !cursor.state().is_terminal()
                || self.pending_prompt
            {
                return Err(HostError::Intent);
            }
            self.pending_terminal = Some(PendingTerminal {
                owner: owner.clone(),
                cursor: cursor.clone(),
            });
        }
        if let Err(error) = self.engine.accept_terminal(&cursor) {
            if matches!(error, HostError::Capacity | HostError::Intent) {
                self.pending_terminal = None;
                self.state = HostState::Running;
            } else {
                self.state = HostState::Recovery;
            }
            return Err(error);
        }
        self.active_turn = None;
        self.pending_cancel = None;
        self.pending_terminal = None;
        self.state = HostState::Idle;
        Ok(HostReply::Outcome(HostOutcome::TurnTerminal))
    }

    fn begin_close(&mut self, intent: HostCloseIntent) -> Result<HostReply, HostError> {
        let owner = intent.owner();
        if !matches!(self.state, HostState::Idle | HostState::Running)
            || !self.matches_owner(&owner)
        {
            return Err(HostError::Intent);
        }
        let prior = self.state;
        self.state = HostState::Draining;
        self.pending_close = Some(PendingClose {
            intent: intent.clone(),
            phase: ClosePhase::Begin,
        });
        match self.engine.begin_close(&intent) {
            Ok(()) => {
                self.pending_close
                    .as_mut()
                    .expect("close intent retained")
                    .phase = ClosePhase::Finish;
                Ok(HostReply::Outcome(HostOutcome::CloseStarted))
            }
            Err(error) => {
                if matches!(error, HostError::Capacity | HostError::Intent) {
                    self.pending_close = None;
                    self.state = prior;
                } else {
                    self.state = HostState::Recovery;
                }
                Err(error)
            }
        }
    }

    fn finish_close(&mut self, intent: HostCloseIntent) -> Result<HostReply, HostError> {
        if self.state != HostState::Draining
            || !self.pending_close.as_ref().is_some_and(|pending| {
                pending.intent == intent && pending.phase == ClosePhase::Finish
            })
        {
            return Err(HostError::Intent);
        }
        let result = self
            .engine
            .converge_zero(&intent)
            .and_then(|()| self.engine.finalize_close(&intent));
        match result {
            Ok(()) => {
                self.owner = None;
                self.active_turn = None;
                self.pending_prompt = false;
                self.pending_cancel = None;
                self.pending_terminal = None;
                self.pending_close = None;
                self.state = HostState::Cold;
                Ok(HostReply::Outcome(HostOutcome::Closed))
            }
            Err(error) => {
                self.state = HostState::Recovery;
                Err(error)
            }
        }
    }

    fn recover_close(&mut self, intent: HostCloseIntent) -> Result<HostReply, HostError> {
        if self.state != HostState::Recovery
            || !self
                .pending_close
                .as_ref()
                .is_some_and(|pending| pending.intent == intent)
        {
            return Err(HostError::Intent);
        }
        self.state = HostState::Draining;
        if self
            .pending_close
            .as_ref()
            .is_some_and(|pending| pending.phase == ClosePhase::Begin)
        {
            if let Err(error) = self.engine.begin_close(&intent) {
                self.state = HostState::Recovery;
                return Err(error);
            }
            self.pending_close
                .as_mut()
                .expect("close intent retained")
                .phase = ClosePhase::Finish;
        }
        self.finish_close(intent)
    }

    fn shutdown(&mut self, deadline: Instant) -> Result<HostReply, HostError> {
        self.state = HostState::Draining;
        match self.engine.shutdown(deadline) {
            Ok(()) => {
                self.owner = None;
                self.active_turn = None;
                self.pending_prompt = false;
                self.pending_cancel = None;
                self.pending_terminal = None;
                self.pending_close = None;
                self.state = HostState::Closed;
                Ok(HostReply::Outcome(HostOutcome::Shutdown))
            }
            Err(error) => {
                self.state = HostState::Recovery;
                Err(error)
            }
        }
    }

    fn matches_owner(&self, owner: &RuntimeOwner) -> bool {
        self.owner.as_ref() == Some(owner)
    }
}
