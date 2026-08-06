use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

use crate::acp::{
    contract::{ClientIntentId, RequiredText},
    intent::Prompt,
    lifecycle::{ItemLifecycle, TurnState},
    process::{StdinEndpoint, TreeCapacityPool, TreeControl, TreeExit},
    projection::ItemContent,
    runtime::{
        ActiveCloseBeginState, AdapterPump, BlockingStdoutReader, BoundedThread, DriverError,
        DriverMailbox, JournalHandle, LiveWrite, ProcessAdapterDriver, ProjectionMailbox,
        SqliteAdapterBatchJournal, TeardownCause, TreeLeaseOwner,
    },
    scheduler::{RuntimeOwner, Scheduler, TreeLease},
    store::{CloseSessionCommand, SendObservation, load_chat_page},
};

use super::child::runtime_fixture::{FixtureClock, FixtureMapper, owner, pump_intent};
use super::child::{ActivityFixtureAdapter, BINDING_DIGEST, PermissionStoreFixture};
use super::{
    external_fixture_adapters::{ExternalCase, fixture_artifact, launch_spec},
    external_fixture_runtime::{ExternalPrepared, prepare_external_case},
    external_fixture_support::ProcessCleanup,
    test_support::{FixtureDeadline, JournalCleanup, launch_with_deadline},
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn turn_text_success_runs_through_supervisor_driver_and_sqlite() {
    run_external_case(ExternalCase::TurnTextSuccess);
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn native_activity_commit_and_failure_hold_the_real_child_through_teardown() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for case_id in [
            "native-activity-child-text",
            "native-activity-foreign-parent",
        ] {
            let observed = run_path_bound_activity(case_id).await;
            let case = super::generated_cases::FROZEN_CASES
                .iter()
                .find(|case| case.id == case_id)
                .unwrap();
            super::oracle::assert_literal(case, observed);
        }
    });
}

struct FixtureTreeLease(TreeLease);

impl TreeLeaseOwner for FixtureTreeLease {
    fn runtime_owner(&self) -> &RuntimeOwner {
        self.0.owner()
    }
}

type ActivityDriver = ProcessAdapterDriver<
    ActivityFixtureAdapter,
    SqliteAdapterBatchJournal,
    FixtureMapper,
    StdinEndpoint,
    TreeControl,
    FixtureTreeLease,
>;

enum ActivityOutcome {
    Child { new_tree: bool },
    Foreign { protocol_violation: bool },
}

async fn run_path_bound_activity(case_id: &'static str) -> serde_json::Value {
    let deadline = FixtureDeadline::new();
    let fixture = PermissionStoreFixture::new();
    let initial_sessions = fixture.session_count();
    let owner = owner(&fixture);
    let intent = pump_intent(&fixture, "message-activity");
    let scheduler = Scheduler::new();
    let lease = scheduler.reserve_tree(owner.clone()).unwrap();
    let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
    let spec = launch_spec(
        case_id,
        fixture.runtime_epoch(),
        fixture_path,
        artifact_digest,
        fixture_byte_len,
    );
    let process_capacity = TreeCapacityPool::isolated_ade_0006a();
    let (journal, runner) = JournalHandle::new(fixture.database_store());
    let journal_cleanup = JournalCleanup::spawn(deadline, journal.clone(), runner).unwrap();
    let process_lease = process_capacity
        .try_acquire(spec.limits.descendants)
        .unwrap();
    let tree = launch_with_deadline(spec, process_lease, deadline).unwrap();
    let (stdin, stdout, control) = tree.into_parts();
    let retained_control = control.clone();
    let mut process_cleanup = ProcessCleanup::new(control.clone(), None, deadline);
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let reader_mailbox = Arc::clone(&mailbox);
    let reader = BoundedThread::spawn("activity-fixture-reader", move || {
        BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
    })
    .unwrap();
    process_cleanup.attach_reader(reader);
    let adapter = ActivityFixtureAdapter::new(
        BINDING_DIGEST,
        fixture.runtime_epoch(),
        fixture.session_id(),
        intent.active_turn.client_message_id.clone(),
    );
    let pump = AdapterPump::new(
        adapter,
        SqliteAdapterBatchJournal::new(journal.clone(), intent.clone()),
        FixtureMapper,
        Arc::new(ProjectionMailbox::default()),
        intent,
    )
    .unwrap();
    let clock = Arc::new(FixtureClock::new(10));
    let mut driver: ActivityDriver = ProcessAdapterDriver::new(
        pump,
        stdin,
        control,
        FixtureTreeLease(lease),
        owner.clone(),
        clock.clone(),
    )
    .unwrap();

    let (outcome, zero_started) = if case_id == "native-activity-child-text" {
        driver
            .dispatch_prompt(
                Prompt::new("prompt", "fixture activity").unwrap(),
                RequiredText::new("message", "message-activity").unwrap(),
            )
            .unwrap();
        for _ in 0..4 {
            if fixture.activity_count() == 1 {
                break;
            }
            tokio::time::timeout(
                deadline.operation_before_cleanup(),
                driver.run_next(mailbox.as_ref()),
            )
            .await
            .expect("activity fixture driver wake timed out")
            .unwrap();
            clock.set(100);
        }
        assert!(fixture.has_projected_activity_text(
            "fixture-child-activity-1",
            "fixture-parent-tool-1",
            "fixture child activity",
        ));
        assert_eq!(fixture.activity_count(), 1);
        assert_eq!(retained_control.active_processes().unwrap(), 1);
        let new_tree = scheduler
            .active()
            .is_none_or(|allocation| allocation.tree != owner);
        assert!(!new_tree);
        let zero_started = Instant::now();
        driver.request_host_termination().unwrap();
        let handoff = driver
            .take_zero_process_handoff_within(deadline.tree_since(zero_started))
            .unwrap();
        assert_eq!(handoff.cause, TeardownCause::HostTermination);
        assert_eq!(handoff.exit, TreeExit::Cancelled);
        handoff.lease.0.zero_process_converged().unwrap();
        (ActivityOutcome::Child { new_tree }, zero_started)
    } else {
        tokio::time::timeout(
            deadline.operation_before_cleanup(),
            driver.run_next(mailbox.as_ref()),
        )
        .await
        .expect("foreign-parent fixture frame timed out")
        .unwrap();
        assert_eq!(retained_control.active_processes().unwrap(), 1);
        clock.set(100);
        let zero_started = Instant::now();
        let error = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            driver.run_next(mailbox.as_ref()),
        )
        .await
        .expect("foreign-parent durable rejection timed out")
        .unwrap_err();
        assert_eq!(
            error,
            DriverError::TeardownRequired(TeardownCause::PumpFailure)
        );
        let handoff = driver
            .take_zero_process_handoff_within(deadline.tree_since(zero_started))
            .unwrap();
        assert_eq!(handoff.cause, TeardownCause::PumpFailure);
        assert_eq!(handoff.exit, TreeExit::ProtocolFailure);
        handoff.lease.0.zero_process_converged().unwrap();
        (
            ActivityOutcome::Foreign {
                protocol_violation: fixture.activity_count() == 0,
            },
            zero_started,
        )
    };

    process_cleanup
        .finish_after_zero_within(deadline.tree_since(zero_started))
        .unwrap();
    assert!(scheduler.active().is_none());
    assert_eq!(process_capacity.usage(), (0, 0));
    journal_cleanup.finish().unwrap();
    if case_id == "native-activity-foreign-parent" {
        assert_eq!(fixture.recover(200).recovered_sessions, 1);
        assert_eq!(fixture.session_state(), "failed");
    }
    let new_session = fixture.session_count() != initial_sessions;
    match outcome {
        ActivityOutcome::Child { new_tree } => serde_json::json!({
            "itemOrigin":"harnessActivity",
            "newSageSession":new_session,
            "newProcessTree":new_tree,
        }),
        ActivityOutcome::Foreign { protocol_violation } => serde_json::json!({
            "terminal":fixture.session_state(),
            "protocolViolation":protocol_violation,
            "newSageSession":new_session,
        }),
    }
}

fn run_external_case(case: ExternalCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let deadline = FixtureDeadline::new();
        let ExternalPrepared {
            temp_root,
            database,
            project_id,
            chat_id,
            started,
            epoch,
            now_millis: now,
            publisher,
            journal: _,
            journal_cleanup,
            scheduler,
            process_capacity,
            mut process_cleanup,
            mailbox,
            active,
            ..
        } = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            prepare_external_case(case, deadline),
        )
        .await
        .expect("fixture preparation timed out");
        let (active, write) = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            active.dispatch_once(now),
        )
        .await
        .expect("fixture prompt dispatch timed out")
        .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
        assert_eq!(write, LiveWrite::Complete);
        let mut active = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            active.observe(SendObservation::CompleteBytes, now + 1),
        )
        .await
        .expect("fixture send observation timed out")
        .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));
        while active
            .turn
            .driver
            .latest_durable_turn_cursor()
            .is_none_or(|cursor| !cursor.state().is_terminal())
        {
            tokio::time::timeout(
                deadline.tree_before_cleanup(),
                active.turn.driver.run_next(mailbox.as_ref()),
            )
            .await
            .expect("fixture driver terminal wake timed out")
            .unwrap();
        }
        process_cleanup.wait_zero();
        assert!(publisher.terminal_seen_after_commit.load(Ordering::SeqCst));

        let page = load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let turn = page.turns.as_slice().last().unwrap();
        assert_eq!(turn.state, TurnState::Completed);
        assert_eq!(turn.items.as_slice().len(), 2);
        let assistant = turn.items.as_slice().last().unwrap();
        assert_eq!(assistant.lifecycle, ItemLifecycle::Completed);
        assert!(matches!(
            &assistant.content,
            ItemContent::Text { text, .. } if text.as_str() == "fixture text"
        ));
        let fixture_case = super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == "turn-text-success")
            .unwrap();
        super::oracle::assert_literal(
            fixture_case,
            serde_json::json!({
                "turn": if turn.state == TurnState::Completed {
                    "completed"
                } else {
                    "incomplete"
                },
                "item": if assistant.lifecycle == ItemLifecycle::Completed {
                    "completed"
                } else {
                    "incomplete"
                },
                "commitBeforeNotify": publisher
                    .terminal_seen_after_commit
                    .load(Ordering::SeqCst),
            }),
        );

        let close = CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::parse("fixture-close").unwrap(),
            session_id: started.session_id,
            runtime_epoch: epoch,
            now_millis: now + 2,
        };
        let closing = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            ActiveCloseBeginState::new(active, close.clone()).begin(),
        )
        .await
        .expect("fixture close begin timed out")
        .unwrap_or_else(|_| panic!("begin fixture close"));
        let zero = tokio::time::timeout(
            deadline.tree_before_cleanup(),
            closing.converge_zero(1, now + 3),
        )
        .await
        .expect("fixture close convergence timed out")
        .unwrap_or_else(|_| panic!("converge fixture process tree"));
        let receipt = zero.receipt().clone();
        let closed = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            zero.finalize(&close, &receipt),
        )
        .await
        .expect("fixture close finalization timed out")
        .unwrap_or_else(|_| panic!("finalize fixture close"));
        drop(closed);
        assert!(scheduler.active().is_none());
        assert_eq!(process_capacity.usage(), (0, 0));
        journal_cleanup.finish().unwrap();
        drop(scheduler);
        drop(publisher);
        drop(mailbox);
        drop(process_cleanup);
        drop(database);
        temp_root.remove();
    });
}
