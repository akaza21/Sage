use std::time::Duration;

use crate::{
    acp::{
        lifecycle::DeliveryState,
        recovery::recover_after_restart,
        runtime::{DriverError, LiveWrite},
        store::SendObservation,
    },
    persistence::Database,
};

use super::{
    external_fixture_adapters::{ExternalCase, send_started},
    external_fixture_runtime::{ExternalPrepared, prepare_external_case},
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
        let prepared = prepare_external_case(case).await;
        match case {
            ExternalCase::ZeroByteFailure => observe_zero_bytes(prepared).await,
            ExternalCase::CompleteDeliveryExit(_) => observe_complete_exit(prepared).await,
            ExternalCase::TurnTextSuccess => unreachable!(),
        }
    });
}

async fn observe_zero_bytes(prepared: ExternalPrepared) {
    let ExternalPrepared {
        case_id,
        root,
        database_path,
        database,
        started,
        now_millis: now,
        publisher,
        journal,
        journal_task,
        scheduler,
        process_capacity,
        retained_control,
        mailbox,
        active,
        ..
    } = prepared;
    let mut attempts = 0_u64;
    let (active, first) = active
        .dispatch_once(now)
        .await
        .unwrap_or_else(|failure| panic!("dispatch zero-byte prompt: {:?}", failure.error));
    attempts += 1;
    assert_eq!(first, LiveWrite::ZeroBytes);
    let active = active
        .observe(SendObservation::ZeroBytes, now + 1)
        .await
        .unwrap_or_else(|failure| panic!("observe zero-byte prompt: {:?}", failure.error));
    let (active, retry) = active
        .dispatch_once(now + 2)
        .await
        .unwrap_or_else(|failure| panic!("retry zero-byte prompt: {:?}", failure.error));
    attempts += 1;
    assert_eq!(retry, LiveWrite::ZeroBytes);
    let active = active
        .observe(SendObservation::ZeroBytes, now + 3)
        .await
        .unwrap_or_else(|failure| panic!("observe zero-byte retry: {:?}", failure.error));
    let active = active
        .observe(SendObservation::ProcessLost, now + 4)
        .await
        .unwrap_or_else(|failure| panic!("terminalize exhausted retry: {:?}", failure.error));
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
    let _ = retained_control.wait_zero().unwrap();
    assert_eq!(process_capacity.usage(), (0, 0));
    drop(active);
    drop(scheduler);
    drop(publisher);
    drop(mailbox);
    drop(retained_control);
    journal.close_when_drained();
    journal_task.await.unwrap();
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
    std::fs::remove_dir_all(root).unwrap();
}

async fn observe_complete_exit(prepared: ExternalPrepared) {
    let ExternalPrepared {
        case_id,
        root,
        database_path,
        database,
        started,
        now_millis: now,
        publisher,
        journal,
        journal_task,
        scheduler,
        process_capacity,
        retained_control,
        mailbox,
        reader,
        active,
        ..
    } = prepared;
    let (active, write) = active
        .dispatch_once(now)
        .await
        .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
    assert_eq!(write, LiveWrite::Complete);
    let mut active = active
        .observe(SendObservation::CompleteBytes, now + 1)
        .await
        .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));
    let mut teardown_observed = false;
    for _ in 0..4 {
        let wake = tokio::time::timeout(
            Duration::from_secs(5),
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
    let _ = retained_control.wait_zero().unwrap();
    reader.expect("delivery reader").join().unwrap();
    assert_eq!(process_capacity.usage(), (0, 0));
    let before = send_started(&database, &started.session_id);
    drop(active);
    drop(scheduler);
    drop(publisher);
    drop(mailbox);
    drop(retained_control);
    journal.close_when_drained();
    journal_task.await.unwrap();
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
    std::fs::remove_dir_all(root).unwrap();
}
