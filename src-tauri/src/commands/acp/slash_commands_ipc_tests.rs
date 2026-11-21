use super::*;
use crate::acp::{
    goal::GoalState,
    ipc::AcpConflictResource,
    store::{
        SendObservation, TurnMutationCommand, apply_turn_terminal, begin_send, prepare_turn,
        record_send_observation,
    },
};
use sage_acp_protocol::lifecycle::{DeliveryState, TurnState};
use std::{future::Future, task::Poll};

#[tokio::test]
async fn a_goal_command_racing_with_turn_completion_is_not_dispatched() {
    let (state, request, _) = super::super::goal::integration_tests::fixture(Ok(()));
    let owner = RuntimeOwner {
        project_id: request.project_id.clone(),
        chat_id: request.chat_id.clone(),
        session_id: request.session_id.clone(),
        runtime_epoch: request.runtime_epoch,
    };
    let commands = state.acp.available_commands(&owner).unwrap();
    commands
        .replace(Ok(vec![
            AvailableCommand::from_acp(&serde_json::json!({
                "name":"goal", "description":"Set a goal", "_meta":{"source":"builtin"}
            }))
            .unwrap(),
        ]))
        .unwrap();
    let mut turn = {
        let mut connection = state.database.connection();
        let prepared = prepare_turn(
            &mut connection,
            PrepareTurnCommand {
                project_id: request.project_id.clone(),
                project_generation: request.project_generation,
                chat_id: request.chat_id.clone(),
                session_id: request.session_id.clone(),
                runtime_epoch: request.runtime_epoch,
                expected_projection_revision: request.expected_projection_revision,
                client_intent_id: sage_acp_protocol::contract::ClientIntentId::parse(
                    "running-turn",
                )
                .unwrap(),
                prompt: Prompt::new("text", "Work in progress").unwrap(),
                now_millis: 4,
            },
        )
        .unwrap();
        let mut turn = TurnMutationCommand {
            project_id: request.project_id.clone(),
            chat_id: request.chat_id.clone(),
            session_id: request.session_id.clone(),
            runtime_epoch: request.runtime_epoch,
            turn_id: prepared.turn_id,
            expected_revision: prepared.turn_revision,
            now_millis: 5,
        };
        turn.expected_revision = begin_send(&mut connection, turn.clone()).unwrap().revision;
        turn.expected_revision =
            record_send_observation(&mut connection, turn.clone(), SendObservation::Correlated)
                .unwrap()
                .revision;
        turn
    };
    // Goal preparation captures an active turn, which can finish before its command steers.
    let captured_turn = (turn.turn_id.clone(), turn.expected_revision);
    let page = state
        .acp
        .load_chat_page(request.project_id.clone(), request.chat_id.clone(), None)
        .unwrap();
    let initial_guard = state.project_transitions.lock().await;
    let mut command = std::pin::pin!(run_command_turn(
        &state,
        RunCommandRequest {
            session: ActivateAcpSessionRequest {
                project_id: request.project_id.clone(),
                project_generation: request.project_generation,
                chat_id: request.chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: request.client_intent_id,
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
            },
            expected_commands_revision: commands.snapshot().revision,
            name: RequiredText::new("name", "goal").unwrap(),
            input: Some(RequiredText::new("input", "Ship the release").unwrap()),
        },
        Some(captured_turn),
    ));
    // Queue command-list validation, then completion. The FIFO transition lock
    // lets completion run after list admission but before steering validation.
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(command.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let mut completing = std::pin::pin!(state.project_transitions.lock());
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(completing.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(initial_guard);
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(command.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let completion_guard = completing.await;
    turn.now_millis = 6;
    let terminal = apply_turn_terminal(
        &mut state.database.connection(),
        turn,
        TurnState::Completed,
        DeliveryState::Correlated,
    )
    .unwrap();
    drop(completion_guard);
    let error = command.await.unwrap_err();
    assert_eq!(error.prompt_dispatch, PromptDispatch::NotStarted);
    assert!(
        matches!(
            error.failure,
            AcpIpcError::Conflict {
                resource: AcpConflictResource::TurnState,
                ..
            }
        ),
        "{error:?}"
    );
    let after = state
        .acp
        .load_chat_page(request.project_id, request.chat_id, None)
        .unwrap();
    assert_eq!(after.projection_revision, terminal.projection_revision);
    assert_eq!(after.turns.as_slice().len(), 1);
    assert_eq!(after.turns.as_slice()[0].state, TurnState::Completed);
    assert!(matches!(after.goal, GoalState::Goal { .. }));
    state.acp.shutdown_blocking().unwrap();
}
