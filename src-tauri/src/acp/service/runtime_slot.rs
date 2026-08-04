#![allow(dead_code)] // Removed when native ACP operations claim the production slot.

use std::sync::{Arc, Mutex};

use crate::acp::{runtime::RuntimeHost, scheduler::RuntimeOwner};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeSlotError {
    #[error("the one live runtime slot is occupied")]
    Occupied,
    #[error("the live runtime belongs to a different session epoch")]
    Foreign,
    #[error("runtime shutdown has started")]
    Stopping,
    #[error("the exact runtime owner requires authoritative recovery")]
    Recovery,
}

enum State {
    Empty,
    Claimed {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    },
    Recovery {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    },
    Stopping {
        owner: Option<RuntimeOwner>,
        host: Option<Arc<RuntimeHost>>,
    },
    Stopped,
}

pub(crate) struct RuntimeSlot {
    state: Mutex<State>,
}

impl RuntimeSlot {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State::Empty),
        }
    }

    pub(crate) fn claim(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Empty => {
                *state = State::Claimed { owner, host };
                Ok(())
            }
            State::Claimed { .. } | State::Recovery { .. } => Err(RuntimeSlotError::Occupied),
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
        }
    }

    pub(crate) fn for_owner(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<Arc<RuntimeHost>, RuntimeSlotError> {
        match &*self.lock() {
            State::Claimed {
                owner: current,
                host,
            } if current == owner => Ok(Arc::clone(host)),
            State::Recovery { owner: current, .. } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::Claimed { .. } | State::Recovery { .. } | State::Empty => {
                Err(RuntimeSlotError::Foreign)
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
        }
    }

    pub(crate) fn mark_recovery(&self, owner: &RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        let State::Claimed {
            owner: current,
            host,
        } = &*state
        else {
            return match &*state {
                State::Recovery { owner: current, .. } if current == owner => Ok(()),
                State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
                _ => Err(RuntimeSlotError::Foreign),
            };
        };
        if current != owner {
            return Err(RuntimeSlotError::Foreign);
        }
        *state = State::Recovery {
            owner: current.clone(),
            host: Arc::clone(host),
        };
        Ok(())
    }

    pub(crate) fn active(&self) -> Option<Arc<RuntimeHost>> {
        match &*self.lock() {
            State::Claimed { host, .. } | State::Recovery { host, .. } => Some(Arc::clone(host)),
            State::Empty | State::Stopping { .. } | State::Stopped => None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active().is_some()
    }

    pub(crate) fn begin_shutdown(&self) -> Option<Arc<RuntimeHost>> {
        let mut state = self.lock();
        let retained = match &*state {
            State::Claimed { owner, host } | State::Recovery { owner, host } => {
                Some((owner.clone(), Arc::clone(host)))
            }
            State::Empty | State::Stopping { .. } | State::Stopped => None,
        };
        if matches!(&*state, State::Empty) {
            *state = State::Stopping {
                owner: None,
                host: None,
            };
        } else if let Some((owner, host)) = &retained {
            *state = State::Stopping {
                owner: Some(owner.clone()),
                host: Some(Arc::clone(host)),
            };
        }
        retained.map(|(_, host)| host)
    }

    pub(crate) fn shutdown_failed(&self) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        let State::Stopping {
            owner: Some(owner),
            host: Some(host),
        } = &*state
        else {
            return Err(RuntimeSlotError::Stopping);
        };
        *state = State::Recovery {
            owner: owner.clone(),
            host: Arc::clone(host),
        };
        Ok(())
    }

    pub(crate) fn finish_shutdown(&self) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        if !matches!(&*state, State::Stopping { .. }) {
            return Err(RuntimeSlotError::Recovery);
        }
        *state = State::Stopped;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::acp::{
        contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
        runtime::{
            HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, PromptWrite,
            RuntimeEngine,
        },
        store::AdapterTurnCursor,
    };

    use super::*;

    #[derive(Default)]
    struct Engine(Arc<AtomicUsize>);

    impl RuntimeEngine for Engine {
        fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
            Ok(())
        }
        fn prepare_prompt(
            &mut self,
            _: &HostPromptIntent,
        ) -> Result<crate::acp::contract::SageTurnId, HostError> {
            Err(HostError::Unavailable)
        }
        fn dispatch_prompt(
            &mut self,
            _: &RuntimeOwner,
            _: &crate::acp::contract::SageTurnId,
        ) -> Result<PromptWrite, HostError> {
            Err(HostError::Unavailable)
        }
        fn retry_prompt(
            &mut self,
            _: &RuntimeOwner,
            _: &crate::acp::contract::SageTurnId,
        ) -> Result<PromptWrite, HostError> {
            Err(HostError::Unavailable)
        }
        fn record_delivery_unknown(
            &mut self,
            _: &RuntimeOwner,
            _: &crate::acp::contract::SageTurnId,
        ) -> Result<(), HostError> {
            Ok(())
        }
        fn record_retry_exhausted(
            &mut self,
            _: &RuntimeOwner,
            _: &crate::acp::contract::SageTurnId,
        ) -> Result<(), HostError> {
            Ok(())
        }
        fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
            Ok(())
        }
        fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
            Ok(())
        }
        fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
            Ok(())
        }
        fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
            Ok(())
        }
        fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
            Ok(())
        }
        fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
            Ok(())
        }
        fn shutdown(&mut self) -> Result<(), HostError> {
            match self
                .0
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                }) {
                Ok(_) => Err(HostError::Unavailable),
                Err(_) => Ok(()),
            }
        }
    }

    fn owner(session: &str) -> RuntimeOwner {
        RuntimeOwner {
            project_id: SageProjectId::parse("project-slot").unwrap(),
            chat_id: SageChatId::parse("chat-slot").unwrap(),
            session_id: SageAgentSessionId::parse(session).unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        }
    }

    fn host(owner: &RuntimeOwner) -> Arc<RuntimeHost> {
        host_with_engine(owner, Engine::default())
    }

    fn host_with_engine(owner: &RuntimeOwner, engine: Engine) -> Arc<RuntimeHost> {
        let host = Arc::new(RuntimeHost::start(engine).unwrap());
        host.activate(owner.clone()).unwrap();
        host
    }

    #[test]
    fn claim_never_overwrites_a_live_or_recovery_owner() {
        let slot = RuntimeSlot::new();
        let first = owner("session-first");
        let second = owner("session-second");
        slot.claim(first.clone(), host(&first)).unwrap();
        assert!(matches!(
            slot.claim(second.clone(), host(&second)),
            Err(RuntimeSlotError::Occupied)
        ));
        slot.mark_recovery(&first).unwrap();
        assert!(matches!(
            slot.claim(second.clone(), host(&second)),
            Err(RuntimeSlotError::Occupied)
        ));
        assert!(matches!(
            slot.for_owner(&first),
            Err(RuntimeSlotError::Recovery)
        ));
        assert!(matches!(
            slot.for_owner(&second),
            Err(RuntimeSlotError::Foreign)
        ));
        if let Some(host) = slot.begin_shutdown() {
            host.shutdown().unwrap();
        }
        slot.finish_shutdown().unwrap();
    }

    #[test]
    fn stopping_is_idempotent_and_rejects_late_claims() {
        let slot = RuntimeSlot::new();
        assert!(slot.begin_shutdown().is_none());
        assert!(slot.begin_shutdown().is_none());
        slot.finish_shutdown().unwrap();
        let owner = owner("session-late");
        assert!(matches!(
            slot.claim(owner.clone(), host(&owner)),
            Err(RuntimeSlotError::Stopping)
        ));
    }

    #[test]
    fn failed_shutdown_retains_the_exact_owner_in_recovery_until_retry_succeeds() {
        let slot = RuntimeSlot::new();
        let active = owner("session-recovery");
        let failures = Arc::new(AtomicUsize::new(1));
        slot.claim(
            active.clone(),
            host_with_engine(&active, Engine(Arc::clone(&failures))),
        )
        .unwrap();

        let runtime = slot.begin_shutdown().expect("exact retained host");
        assert_eq!(runtime.shutdown(), Err(HostError::Unavailable));
        slot.shutdown_failed().unwrap();
        assert!(slot.is_active());
        assert!(matches!(
            slot.for_owner(&active),
            Err(RuntimeSlotError::Recovery)
        ));
        assert_eq!(slot.finish_shutdown(), Err(RuntimeSlotError::Recovery));
        assert!(slot.is_active());
        let foreign = owner("session-foreign");
        assert!(matches!(
            slot.claim(foreign.clone(), host(&foreign)),
            Err(RuntimeSlotError::Occupied)
        ));

        slot.begin_shutdown()
            .expect("same host retry")
            .shutdown()
            .unwrap();
        slot.finish_shutdown().unwrap();
        assert!(!slot.is_active());
    }
}
