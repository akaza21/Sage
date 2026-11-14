use super::*;
use crate::acp::store::{StartSessionCommand, start_session};
use sage_acp_protocol::{contract::ProjectionRevision, intent::BindingId};

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
            [sage_core::limits::LIMITS
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

fn workspace_marks(state: &AppState) -> i64 {
    state
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM acp_turn_workspace_changes",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_archive_during_the_unlocked_capture_fails_the_send_as_stale() {
    let root = std::env::temp_dir().join(format!("sage-send-archive-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = crate::persistence::Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let layout = |snapshot: crate::domain::BootstrapSnapshot| crate::domain::ProjectLayoutRequest {
        project_id: project.id.clone(),
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    };
    let chat = initial.active_tab_id.clone().unwrap();
    database.create_chat(&layout(initial)).unwrap();
    let project_id = SageProjectId::parse(&project.id).unwrap();
    let chat_id = SageChatId::parse(chat.clone()).unwrap();
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
    let archive = crate::domain::ChatLifecycleRequest {
        project_id: project.id.clone(),
        chat_id: chat,
        current_layout: layout(database.bootstrap_snapshot().unwrap()),
    };
    let state = AppState::new(database);
    let transition = state
        .lock_active_project(&project_id, ProjectGeneration::ZERO)
        .await
        .unwrap();
    let (entered, capturing) = tokio::sync::oneshot::channel();
    let (archived_tx, archived_rx) = tokio::sync::oneshot::channel();
    let intent = ClientIntentId::new();
    let capture = mark_before_prompt_with(
        &state,
        transition,
        &intent,
        &session.session_id,
        |_| async move {
            entered.send(()).unwrap();
            archived_rx.await.unwrap();
            turn_workspace::BaselineCapture::Unsupported
        },
    );
    let archive = async {
        capturing.await.unwrap();
        let archived = crate::chat_archive_commands::archive_chat_inner(&state, archive, 0).await;
        archived_tx.send(()).unwrap();
        archived
    };
    let ((), archived) = tokio::join!(capture, archive);
    assert!(archived.committed, "{:?}", archived.recovery_error);
    assert_eq!(workspace_marks(&state), 1, "the capture saved its mark");

    let relocked = relock_after_capture(
        &state,
        &project_id,
        ProjectGeneration::ZERO,
        &session.session_id,
        &intent,
    )
    .await
    .err();
    assert!(
        matches!(
            relocked,
            Some(AcpIpcError::Conflict {
                resource: crate::acp::ipc::AcpConflictResource::ProjectGeneration,
                ..
            })
        ),
        "the send must fail as stale"
    );
    assert_eq!(
        workspace_marks(&state),
        0,
        "the stale send forgets its mark"
    );
    let turns: i64 = state
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(turns, 0, "no Turn was created");
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
