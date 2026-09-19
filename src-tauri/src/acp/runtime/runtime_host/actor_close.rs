use crate::acp::{runtime::HostError, scheduler::RuntimeOwner};

use super::super::types::{
    ClosePhase, HostCloseIntent, HostOutcome, HostReply, HostState, PendingClose, RuntimeEngine,
};
use super::Actor;

impl<E: RuntimeEngine> Actor<E> {
    pub(super) fn begin_close(&mut self, intent: HostCloseIntent) -> Result<HostReply, HostError> {
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

    pub(super) fn finish_close(&mut self, intent: HostCloseIntent) -> Result<HostReply, HostError> {
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

    pub(super) fn recover_close(
        &mut self,
        intent: HostCloseIntent,
    ) -> Result<HostReply, HostError> {
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

    pub(super) fn matches_owner(&self, owner: &RuntimeOwner) -> bool {
        self.owner.as_ref() == Some(owner)
    }
}
