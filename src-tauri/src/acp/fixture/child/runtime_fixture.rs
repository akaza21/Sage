use std::sync::atomic::{AtomicI64, Ordering};

use crate::acp::{
    bindings::adapter::ResourceMapper,
    contract::RequiredText,
    intent::{ExternalId, SafeResourceRef, UnavailableResourceReason},
    runtime::{ActiveTurnIntent, DriverClock, PumpIntent},
    scheduler::RuntimeOwner,
};

use super::permission_store_fixture::{BINDING_DIGEST, PermissionStoreFixture};

pub(in crate::acp::fixture) struct FixtureMapper;

impl ResourceMapper for FixtureMapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

pub(in crate::acp::fixture) struct FixtureClock(AtomicI64);

impl FixtureClock {
    pub(in crate::acp::fixture) fn new(now_millis: i64) -> Self {
        Self(AtomicI64::new(now_millis))
    }
}

impl DriverClock for FixtureClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(in crate::acp::fixture) fn owner(fixture: &PermissionStoreFixture) -> RuntimeOwner {
    RuntimeOwner {
        project_id: fixture.project_id(),
        chat_id: fixture.chat_id(),
        session_id: fixture.session_id(),
        runtime_epoch: fixture.runtime_epoch(),
    }
}

pub(in crate::acp::fixture) fn pump_intent(
    fixture: &PermissionStoreFixture,
    client_message_id: &'static str,
) -> PumpIntent {
    PumpIntent {
        binding_digest: BINDING_DIGEST,
        project_id: fixture.project_id(),
        chat_id: fixture.chat_id(),
        session_id: fixture.session_id(),
        external_session_id: ExternalId::new("externalSession", "fixture-session").unwrap(),
        runtime_epoch: fixture.runtime_epoch(),
        active_turn: ActiveTurnIntent {
            turn_id: fixture.turn_id(),
            external_turn_id: Some(ExternalId::new("externalTurn", "fixture-turn").unwrap()),
            client_message_id: RequiredText::new("message", client_message_id).unwrap(),
        },
    }
}
