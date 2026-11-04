use super::*;
use crate::acp::store::{StartSessionCommand, start_session};
use crate::acp::{contract::ProjectionRevision, intent::BindingId};

#[tokio::test]
async fn optional_baseline_releases_transition_before_waiting_and_never_blocks_send_on_capacity() {
    let root = std::env::temp_dir().join(format!("sage-send-capture-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(&project.id).unwrap();
    let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
    let session = start_session(
        &mut database.connection(),
        StartSessionCommand {
            project_id: project_id.clone(),
            chat_id,
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("binding", "simulator").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: 1,
        },
    )
    .unwrap();
    let state = AppState::new(database);
    let transition = state
        .lock_active_project(&project_id, ProjectGeneration::ZERO)
        .await
        .unwrap();
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let intent = ClientIntentId::new();
    let capture = mark_before_prompt_with(
        &state,
        transition,
        &intent,
        &session.session_id,
        |_| async move {
            entered.send(()).unwrap();
            released.await.unwrap();
            turn_workspace::BaselineCapture::Unsupported
        },
    );
    let other_operation = async {
        waiting.await.unwrap();
        let admitted = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            state.project_transitions.lock(),
        )
        .await;
        release.send(()).unwrap();
        assert!(
            admitted.is_ok(),
            "capture held the project-transition guard"
        );
    };
    tokio::join!(capture, other_operation);
    forget_unprepared(&state, &session.session_id, &intent);
    state
        .database
        .connection()
        .execute(
            "UPDATE acp_store_usage SET shared_payload_bytes=?1",
            [crate::acp::limits::LIMITS
                .durable
                .shared_payload_bytes_global as i64],
        )
        .unwrap();
    let transition = state
        .lock_active_project(&project_id, ProjectGeneration::ZERO)
        .await
        .unwrap();
    mark_before_prompt_with(
        &state,
        transition,
        &ClientIntentId::new(),
        &session.session_id,
        |_| async { panic!("a saturated optional baseline must not capture") },
    )
    .await;
    assert!(state.project_transitions.try_lock().is_ok());
    assert_eq!(
        state
            .database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM acp_turn_workspace_changes",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
