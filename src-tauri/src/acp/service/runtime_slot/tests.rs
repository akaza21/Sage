use std::sync::atomic::{AtomicUsize, Ordering};

use crate::acp::{
    runtime::{
        HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, PromptWrite, RuntimeEngine,
    },
    store::AdapterTurnCursor,
};
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId};

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
    ) -> Result<sage_acp_protocol::contract::SageTurnId, HostError> {
        Err(HostError::Unavailable)
    }

    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::Unavailable)
    }

    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::Unavailable)
    }

    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }

    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
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

    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
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
fn config_overlay_is_cleared_by_exact_slot_lifecycle() {
    let slot = RuntimeSlot::new();
    let first = owner("session-first");
    let first_host = host(&first);
    slot.reserve_start(first.clone()).unwrap();
    slot.config
        .update(
            &first,
            sage_acp_protocol::session_config::SessionConfig::default(),
        )
        .unwrap();
    slot.finish_start(&first, Arc::clone(&first_host)).unwrap();
    assert!(slot.config.snapshot(&first).is_none());
    slot.publish_start(&first).unwrap();
    assert!(slot.config.snapshot(&first).is_some());
    slot.mark_recovery(&owner("foreign")).unwrap_err();
    assert!(slot.config.snapshot(&first).is_some());
    slot.mark_recovery(&first).unwrap();
    let tombstone = slot.config.snapshot(&first).expect("recovery tombstone");
    assert!(tombstone.removed);
    assert_eq!(tombstone.config_revision.get(), 2);
    slot.release_recovered(&first, &first_host).unwrap();
    assert!(slot.config.snapshot(&first).is_none());
    first_host.shutdown(RuntimeHost::close_deadline()).unwrap();
    let second = owner("session-second");
    let second_host = host(&second);
    slot.reserve_start(second.clone()).unwrap();
    slot.config
        .update(
            &second,
            sage_acp_protocol::session_config::SessionConfig::default(),
        )
        .unwrap();
    slot.finish_start(&second, Arc::clone(&second_host))
        .unwrap();
    slot.publish_start(&second).unwrap();
    slot.release(&first, &first_host).unwrap_err();
    assert!(slot.config.snapshot(&second).is_some());
    slot.begin_shutdown();
    assert!(slot.config.snapshot(&second).is_none());
    second_host.shutdown(RuntimeHost::close_deadline()).unwrap();
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
        host.shutdown(RuntimeHost::close_deadline()).unwrap();
    }
    slot.finish_shutdown().unwrap();
}

#[test]
fn start_reservation_serializes_construction_and_preserves_early_failure() {
    let slot = RuntimeSlot::new();
    let first = owner("session-starting");
    let second = owner("session-second");
    slot.reserve_start(first.clone()).unwrap();
    assert_eq!(
        slot.reserve_start(second.clone()),
        Err(RuntimeSlotError::Occupied)
    );
    assert_eq!(
        slot.finish_start(&second, host(&second)),
        Err(RuntimeSlotError::Foreign)
    );
    slot.mark_recovery(&first).unwrap();
    assert_eq!(
        slot.finish_start(&first, host(&first)),
        Err(RuntimeSlotError::Recovery)
    );
    assert_eq!(slot.abandon_start(&first), Err(RuntimeSlotError::Recovery));
    assert!(slot.begin_shutdown().is_none());
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
fn release_requires_the_exact_owner_and_host_then_reopens_the_slot() {
    let slot = RuntimeSlot::new();
    let first = owner("session-first");
    let first_host = host(&first);
    slot.claim(first.clone(), Arc::clone(&first_host)).unwrap();
    let foreign_host = host(&first);
    assert_eq!(
        slot.release(&first, &foreign_host),
        Err(RuntimeSlotError::Foreign)
    );
    foreign_host
        .shutdown(RuntimeHost::close_deadline())
        .unwrap();
    first_host.shutdown(RuntimeHost::close_deadline()).unwrap();
    slot.release(&first, &first_host).unwrap();
    assert!(!slot.is_active());

    let second = owner("session-second");
    let second_host = host(&second);
    slot.claim(second.clone(), Arc::clone(&second_host))
        .unwrap();
    second_host.shutdown(RuntimeHost::close_deadline()).unwrap();
    slot.release(&second, &second_host).unwrap();
}

#[test]
fn primed_host_is_not_addressable_until_runner_ready_is_published() {
    let slot = RuntimeSlot::new();
    let active = owner("session-primed");
    let host = host(&active);
    slot.reserve_start(active.clone()).unwrap();
    slot.finish_start(&active, Arc::clone(&host)).unwrap();
    assert!(matches!(
        slot.for_owner(&active),
        Err(RuntimeSlotError::Foreign)
    ));
    slot.publish_start(&active).unwrap();
    assert!(Arc::ptr_eq(&slot.for_owner(&active).unwrap(), &host));
    host.shutdown(RuntimeHost::close_deadline()).unwrap();
    slot.release(&active, &host).unwrap();
}

#[test]
fn claimed_start_abort_requires_the_exact_host_and_accepts_recovery_state() {
    let slot = RuntimeSlot::new();
    let active = owner("session-start-abort");
    let exact = host(&active);
    let foreign = host(&active);
    slot.reserve_start(active.clone()).unwrap();
    slot.finish_start(&active, Arc::clone(&exact)).unwrap();
    assert_eq!(
        slot.abort_claimed_start(&active, &foreign),
        Err(RuntimeSlotError::Foreign)
    );
    slot.mark_recovery(&active).unwrap();
    exact.abandon_startup().unwrap();
    slot.abort_claimed_start(&active, &exact).unwrap();
    foreign.abandon_startup().unwrap();
    assert!(!slot.is_active());
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
    assert_eq!(
        runtime.shutdown(RuntimeHost::close_deadline()),
        Err(HostError::Unavailable)
    );
    slot.shutdown_failed().unwrap();
    assert!(slot.is_active());
    assert!(matches!(
        slot.for_owner(&active),
        Err(RuntimeSlotError::Recovery)
    ));
    assert_eq!(slot.finish_shutdown(), Err(RuntimeSlotError::Recovery));
    let foreign = owner("session-foreign");
    assert!(matches!(
        slot.claim(foreign.clone(), host(&foreign)),
        Err(RuntimeSlotError::Occupied)
    ));

    slot.begin_shutdown()
        .expect("same host retry")
        .shutdown(RuntimeHost::close_deadline())
        .unwrap();
    slot.finish_shutdown().unwrap();
    assert!(!slot.is_active());
}

#[test]
fn final_window_recovery_joins_exact_host_before_advertising_empty() {
    let slot = RuntimeSlot::new();
    let exact = owner("session-final-window");
    let failures = Arc::new(AtomicUsize::new(1));
    let host = host_with_engine(&exact, Engine(Arc::clone(&failures)));
    slot.claim(exact.clone(), Arc::clone(&host)).unwrap();
    slot.mark_recovery(&exact).unwrap();

    assert_eq!(
        slot.recover_after_zero_process(&exact),
        Err(RuntimeSlotError::Recovery)
    );
    assert!(matches!(
        slot.for_owner(&exact),
        Err(RuntimeSlotError::Recovery)
    ));
    assert!(slot.is_active());

    slot.recover_after_zero_process(&exact).unwrap();
    assert!(!slot.is_active());
    assert!(matches!(
        slot.for_owner(&exact),
        Err(RuntimeSlotError::Foreign)
    ));
}

#[test]
fn explicit_recovery_close_releases_only_the_retained_exact_host() {
    let slot = RuntimeSlot::new();
    let exact_owner = owner("session-explicit-close");
    let exact = host(&exact_owner);
    let foreign = host(&exact_owner);
    slot.claim(exact_owner.clone(), Arc::clone(&exact)).unwrap();
    slot.mark_recovery(&exact_owner).unwrap();

    assert_eq!(
        slot.release_recovered(&exact_owner, &foreign),
        Err(RuntimeSlotError::Foreign)
    );
    assert!(slot.is_active());
    exact.shutdown(RuntimeHost::close_deadline()).unwrap();
    slot.release_recovered(&exact_owner, &exact).unwrap();
    assert!(!slot.is_active());
    foreign.shutdown(RuntimeHost::close_deadline()).unwrap();
}
