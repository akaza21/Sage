use std::time::Instant;

use crate::{
    acp::{
        contract::SageTurnId,
        lifecycle::DeliveryState,
        recovery::recover_after_restart,
        runtime::{
            BoundedThread, DriverError, HostCancelIntent, HostError, HostOutcome, HostPromptIntent,
            HostState, LiveRuntimeEngine, LiveRuntimePort, LiveWrite, RuntimeHost,
        },
        scheduler::RuntimeOwner,
        store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
    },
    persistence::Database,
};

use super::{
    external_fixture_adapters::{ExternalCase, send_started},
    external_fixture_runtime::{ExternalActive, ExternalPrepared, prepare_external_case},
    test_support::FixtureDeadline,
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn complete_delivery_exit_cases_recover_without_new_send() {
    for case_id in [
        "send-complete-uncorrelated-exit",
        "send-correlated-then-exit",
    ] {
        run(ExternalCase::CompleteDeliveryExit(case_id));
    }
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn native_zero_byte_failure_retries_once_without_delivery_unknown() {
    run(ExternalCase::ZeroByteFailure);
}

fn run(case: ExternalCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let deadline = FixtureDeadline::new();
        let prepared = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            prepare_external_case(case, deadline),
        )
        .await
        .expect("delivery fixture preparation timed out");
        match case {
            ExternalCase::ZeroByteFailure => observe_zero_bytes(prepared, deadline).await,
            ExternalCase::CompleteDeliveryExit(_) => {
                observe_complete_exit(prepared, deadline).await
            }
            ExternalCase::TurnTextSuccess | ExternalCase::HandoffInline => unreachable!(),
        }
    });
}

async fn observe_zero_bytes(prepared: ExternalPrepared, deadline: FixtureDeadline) {
    let ExternalPrepared {
        case_id,
        temp_root,
        database_path,
        database,
        project_id,
        chat_id,
        started,
        epoch,
        now_millis: now,
        prompt_intent,
        publisher,
        journal,
        journal_cleanup,
        scheduler,
        process_capacity,
        process_cleanup,
        mailbox,
        active,
        ..
    } = prepared;
    let owner = RuntimeOwner {
        project_id,
        chat_id,
        session_id: started.session_id.clone(),
        runtime_epoch: epoch,
    };
    let host_deadline = deadline.operation_end_before_cleanup();
    assert!(
        host_deadline > Instant::now(),
        "fixture cleanup deadline reached"
    );
    let host = RuntimeHost::start_with_deadline(
        LiveRuntimeEngine::new(PreparedZeroFixturePort {
            owner: owner.clone(),
            prompt_intent: prompt_intent.clone(),
            active: Some(active),
            runtime: tokio::runtime::Handle::current(),
            now_millis: now,
        }),
        host_deadline,
        deadline.end(),
    )
    .unwrap();
    let mut host_thread = BoundedThread::spawn("zero-byte-fixture-host", move || {
        assert_eq!(host.activate(owner), Ok(HostOutcome::Activated));
        assert_eq!(host.send(prompt_intent), Err(HostError::RetryExhausted));
        assert_eq!(host.state(), Ok(HostState::Recovery));
        host.shutdown(RuntimeHost::close_deadline()).unwrap();
    })
    .unwrap();
    let host_result = host_thread.wait(host_deadline.saturating_duration_since(Instant::now()));
    if host_result.is_err() {
        let _ = host_thread.wait(deadline.tree());
    }
    host_result.unwrap();

    let attempts = send_started(&database, &started.session_id);
    assert_eq!(attempts, 2);
    let (state, delivery): (String, String) = database
        .connection()
        .query_row(
            "SELECT state,delivery_state FROM turns WHERE session_id=?1",
            [started.session_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((state.as_str(), delivery.as_str()), ("failed", "zeroBytes"));
    let before = send_started(&database, &started.session_id);
    assert_eq!(process_capacity.usage(), (0, 0));
    drop(scheduler);
    drop(publisher);
    drop(mailbox);
    drop(process_cleanup);
    journal_cleanup.finish().unwrap();
    drop(journal);
    drop(database);

    let reopened = Database::open(&database_path).unwrap();
    recover_after_restart(&mut reopened.connection(), now + 5).unwrap();
    assert_eq!(send_started(&reopened, &started.session_id), before);
    let fixture_case = super::generated_cases::FROZEN_CASES
        .iter()
        .find(|case| case.id == case_id)
        .unwrap();
    super::oracle::assert_literal(
        fixture_case,
        serde_json::json!({
            "retryCount": attempts.saturating_sub(1),
            "deliveryUnknown": state == "deliveryUnknown" || delivery == "possibleBytes",
        }),
    );
    drop(reopened);
    temp_root.remove();
}

struct PreparedZeroFixturePort {
    owner: RuntimeOwner,
    prompt_intent: HostPromptIntent,
    active: Option<ExternalActive>,
    runtime: tokio::runtime::Handle,
    now_millis: i64,
}

impl PreparedZeroFixturePort {
    fn next_millis(&mut self) -> i64 {
        let now = self.now_millis;
        self.now_millis = self.now_millis.checked_add(1).unwrap();
        now
    }

    fn exact_turn(&self, owner: &RuntimeOwner, turn_id: &SageTurnId) -> bool {
        owner == &self.owner
            && self
                .active
                .as_ref()
                .is_some_and(|active| active.turn.dispatch.turn_id() == turn_id)
    }
}

impl LiveRuntimePort for PreparedZeroFixturePort {
    type ZeroProof = ();

    fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        if intent != &self.prompt_intent || intent.owner() != self.owner {
            return Err(HostError::Intent);
        }
        self.active
            .as_ref()
            .map(|active| active.turn.dispatch.turn_id().clone())
            .ok_or(HostError::State)
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<LiveWrite, HostError> {
        if !self.exact_turn(owner, turn_id) {
            return Err(HostError::Intent);
        }
        let active = self.active.take().ok_or(HostError::State)?;
        let now = self.next_millis();
        let (active, write) = self
            .runtime
            .block_on(active.dispatch_once(now))
            .unwrap_or_else(|failure| panic!("dispatch zero-byte prompt: {:?}", failure.error));
        self.active = Some(active);
        Ok(write)
    }

    fn observe_send(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
        observation: SendObservation,
    ) -> Result<(), HostError> {
        if !self.exact_turn(owner, turn_id) {
            return Err(HostError::Intent);
        }
        let active = self.active.take().ok_or(HostError::State)?;
        let now = self.next_millis();
        let active = self
            .runtime
            .block_on(active.observe(observation, now))
            .unwrap_or_else(|failure| panic!("observe zero-byte prompt: {:?}", failure.error));
        self.active = Some(active);
        Ok(())
    }

    fn begin_cancel(&mut self, _intent: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn dispatch_cancel(&mut self, _intent: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn accept_terminal(&mut self, _cursor: &AdapterTurnCursor) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn begin_close(&mut self, _command: &CloseSessionCommand) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn wait_zero(&mut self, _owner: &RuntimeOwner) -> Result<Self::ZeroProof, HostError> {
        Err(HostError::State)
    }

    fn finalize_close(
        &mut self,
        _command: &CloseSessionCommand,
        proof: Self::ZeroProof,
    ) -> Result<(), (HostError, Self::ZeroProof)> {
        Err((HostError::State, proof))
    }

    fn shutdown(&mut self, _deadline: Instant) -> Result<(), HostError> {
        self.active.take();
        Ok(())
    }
}

async fn observe_complete_exit(prepared: ExternalPrepared, deadline: FixtureDeadline) {
    let ExternalPrepared {
        case_id,
        temp_root,
        database_path,
        database,
        started,
        now_millis: now,
        publisher,
        journal,
        journal_cleanup,
        scheduler,
        process_capacity,
        mut process_cleanup,
        mailbox,
        active,
        ..
    } = prepared;
    let (active, write) = tokio::time::timeout(
        deadline.operation_before_cleanup(),
        active.dispatch_once(now),
    )
    .await
    .expect("delivery fixture prompt dispatch timed out")
    .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
    assert_eq!(write, LiveWrite::Complete);
    let mut active = tokio::time::timeout(
        deadline.operation_before_cleanup(),
        active.observe(SendObservation::CompleteBytes, now + 1),
    )
    .await
    .expect("delivery fixture send observation timed out")
    .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));
    let mut teardown_observed = false;
    for _ in 0..4 {
        let wake = tokio::time::timeout(
            deadline.tree_before_cleanup(),
            active.turn.driver.run_next(mailbox.as_ref()),
        )
        .await
        .expect("delivery fixture driver wake timed out");
        match wake {
            Ok(_) => {}
            Err(DriverError::TeardownRequired(_)) => {
                teardown_observed = true;
                break;
            }
            Err(error) => panic!("delivery fixture driver failed unexpectedly: {error:?}"),
        }
    }
    assert!(
        teardown_observed,
        "delivery fixture did not exit in four wakes"
    );
    let correlated = active
        .turn
        .driver
        .latest_durable_turn_cursor()
        .is_some_and(|cursor| cursor.delivery_state() == DeliveryState::Correlated);
    process_cleanup.wait_zero();
    assert_eq!(process_capacity.usage(), (0, 0));
    let before = send_started(&database, &started.session_id);
    drop(active);
    drop(scheduler);
    drop(publisher);
    drop(mailbox);
    drop(process_cleanup);
    journal_cleanup.finish().unwrap();
    drop(journal);
    drop(database);

    let reopened = Database::open(&database_path).unwrap();
    recover_after_restart(&mut reopened.connection(), now + 2).unwrap();
    let after = send_started(&reopened, &started.session_id);
    assert_eq!(after, before);
    let (state, delivery): (String, String) = reopened
        .connection()
        .query_row(
            "SELECT state,delivery_state FROM turns WHERE session_id=?1",
            [started.session_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let expected = if correlated {
        ("interrupted", "correlated")
    } else {
        ("deliveryUnknown", "completeBytes")
    };
    assert_eq!((state.as_str(), delivery.as_str()), expected);
    let fixture_case = super::generated_cases::FROZEN_CASES
        .iter()
        .find(|case| case.id == case_id)
        .unwrap();
    super::oracle::assert_literal(
        fixture_case,
        if correlated {
            serde_json::json!({"turn":"interrupted","deliveryUnknown":false})
        } else {
            serde_json::json!({"turn":"deliveryUnknown","automaticReplay":after > before})
        },
    );
    drop(reopened);
    temp_root.remove();
}
