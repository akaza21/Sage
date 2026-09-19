use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::acp::{scheduler::RuntimeOwner, store::AdapterTurnCursor};

#[path = "actor_close.rs"]
mod actor_close;
#[path = "actor_park.rs"]
mod actor_park;
#[path = "actor_session_config.rs"]
mod actor_session_config;

use super::{
    mailbox::{HostMailbox, HostMessage},
    types::{
        CancelPhase, HostCancelIntent, HostCommand, HostError, HostOutcome, HostPromptIntent,
        HostReply, HostState, PendingCancel, PendingClose, PendingHostConfig, PendingTerminal,
        PromptWrite, RuntimeEngine,
    },
};

pub(super) struct Actor<E> {
    pub(super) engine: E,
    pub(super) state: HostState,
    pub(super) owner: Option<RuntimeOwner>,
    pub(super) active_turn: Option<crate::acp::contract::SageTurnId>,
    pub(super) pending_prompt: bool,
    pub(super) pending_cancel: Option<PendingCancel>,
    pub(super) pending_terminal: Option<PendingTerminal>,
    pub(super) pending_close: Option<PendingClose>,
    pub(super) pending_config: Option<PendingHostConfig>,
    pub(super) next_config_operation: u64,
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
            pending_config: None,
            next_config_operation: 0,
        }
    }

    pub(super) fn run(mut self, mailbox: Arc<HostMailbox>) {
        while let Some(message) =
            mailbox.wait_next(self.pending_config.as_ref().map(|pending| pending.deadline))
        {
            match message {
                HostMessage::ConfigCompletion(completion) => {
                    self.complete_config(completion);
                }
                HostMessage::ConfigDeadline => {
                    if let Some(deadline) =
                        self.pending_config.as_ref().map(|pending| pending.deadline)
                    {
                        self.engine.abandon_configuration(deadline);
                    }
                    self.fail_pending_config(HostError::Deadline);
                }
                HostMessage::Command(envelope) => {
                    if matches!(envelope.command, HostCommand::ConfigureSession(_)) {
                        self.begin_configure_session(envelope, &mailbox);
                        continue;
                    }
                    let stop = matches!(&envelope.command, HostCommand::Shutdown);
                    let abandons_config = matches!(
                        &envelope.command,
                        HostCommand::BeginClose(_) | HostCommand::BeginPark | HostCommand::Shutdown
                    );
                    if abandons_config && self.pending_config.is_some() {
                        self.engine.abandon_configuration(envelope.deadline);
                        self.settle_pending_config(HostError::Closed);
                    }
                    // Defense in depth: mailbox dequeue and actor dispatch are
                    // separate scheduling points, so queued expiry must be checked at
                    // both boundaries using the caller's original absolute deadline.
                    let result = if Instant::now() >= envelope.deadline {
                        Err(HostError::Deadline)
                    } else {
                        self.dispatch(envelope.command, envelope.deadline)
                    };
                    let stopped =
                        stop && matches!(result, Ok(HostReply::Outcome(HostOutcome::Shutdown)));
                    let _ = envelope.reply.send(result);
                    if stopped {
                        self.fail_pending_config(HostError::Closed);
                        mailbox.close();
                        return;
                    }
                }
            }
        }
        self.fail_pending_config(HostError::Closed);
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
            HostCommand::ConfigureSession(_) => Err(HostError::State),
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
            HostCommand::AcceptDurableCompleted => self.accept_durable_completed(),
            HostCommand::BeginPark => self.begin_park(),
            HostCommand::FinishPark => self.finish_park(deadline),
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
}
