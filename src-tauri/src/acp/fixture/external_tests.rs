use std::sync::atomic::Ordering;

use crate::acp::{
    contract::ClientIntentId,
    lifecycle::{ItemLifecycle, TurnState},
    projection::ItemContent,
    runtime::{ActiveCloseBeginState, LiveWrite},
    store::{CloseSessionCommand, SendObservation, load_chat_page},
};

use super::{
    external_fixture_adapters::ExternalCase,
    external_fixture_runtime::{ExternalPrepared, prepare_external_case},
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn turn_text_success_runs_through_supervisor_driver_and_sqlite() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let ExternalPrepared {
            temp_root,
            database,
            project_id,
            chat_id,
            started,
            epoch,
            now_millis: now,
            publisher,
            journal,
            journal_task,
            scheduler,
            process_capacity,
            mut process_cleanup,
            mailbox,
            active,
            ..
        } = prepare_external_case(ExternalCase::TurnTextSuccess).await;
        let (active, write) = active
            .dispatch_once(now)
            .await
            .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
        assert_eq!(write, LiveWrite::Complete);
        let mut active = active
            .observe(SendObservation::CompleteBytes, now + 1)
            .await
            .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));
        while active
            .turn
            .driver
            .latest_durable_turn_cursor()
            .is_none_or(|cursor| !cursor.state().is_terminal())
        {
            active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
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
        let closing = ActiveCloseBeginState::new(active, close.clone())
            .begin()
            .await
            .unwrap_or_else(|_| panic!("begin fixture close"));
        let zero = closing
            .converge_zero(1, now + 3)
            .await
            .unwrap_or_else(|_| panic!("converge fixture process tree"));
        let receipt = zero.receipt().clone();
        let closed = zero
            .finalize(&close, &receipt)
            .await
            .unwrap_or_else(|_| panic!("finalize fixture close"));
        drop(closed);
        assert!(scheduler.active().is_none());
        assert_eq!(process_capacity.usage(), (0, 0));
        journal.close_when_drained();
        journal_task.await.unwrap();
        drop(scheduler);
        drop(publisher);
        drop(mailbox);
        drop(process_cleanup);
        drop(database);
        temp_root.remove();
    });
}
