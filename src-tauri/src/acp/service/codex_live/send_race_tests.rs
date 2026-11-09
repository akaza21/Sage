//! A send races session events through the production send path: the
//! renderer read the chat at one projection revision, and session facts
//! committed before its send reached the coordinator.

use std::{path::Path, sync::Arc};

use super::DatabaseAuthority;
use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RequiredText, RuntimeEpoch,
            SageChatId, SageProjectId, SchemaVersionV1,
        },
        intent::{
            BindingId, CapabilitySnapshotV1, ExternalId, ModelId, Prompt,
            session_event::{
                SessionActivityState, SessionNoticeLevel, SessionScopedEvent,
                fixtures::{activity, notice, title},
            },
        },
        lifecycle::TurnState,
        runtime::{
            ActiveDispatch, CoordinatorError, JournalHandle, PrepareDispatch, ProjectionMailbox,
            SendCoordinator,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            ActivateSessionCommand, AdapterInboundBatchCommand, AdapterInboundEvent,
            AdapterStoreOwner, CompleteActivationCommand, DatabaseAcpStore, PrepareTurnCommand,
            SessionEventCommand, StartSessionCommand, TurnMutationCommand, activate_session,
            apply_adapter_inbound_batch, begin_send, complete_activation, prepare_turn,
            start_session,
        },
    },
    persistence::Database,
};

struct IdleSession {
    database: Database,
    owner: AdapterStoreOwner,
    root: std::path::PathBuf,
}

fn projection(connection: &rusqlite::Connection, chat_id: &SageChatId) -> ProjectionRevision {
    let revision: i64 = connection
        .query_row(
            "SELECT projection_revision FROM chats WHERE id=?1",
            [chat_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    ProjectionRevision::new(revision as u64).unwrap()
}

fn capabilities() -> String {
    serde_json::to_string(&CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "fixture-v1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 1,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    })
    .unwrap()
}

fn apply(database: &Database, owner: &AdapterStoreOwner, events: Vec<AdapterInboundEvent>) {
    let batch = AdapterInboundBatchCommand::new(owner.clone(), 1024, 4096, events).unwrap();
    apply_adapter_inbound_batch(&mut database.connection(), batch).unwrap();
}

fn commit(database: &Database, owner: &AdapterStoreOwner, event: SessionScopedEvent) {
    let command = SessionEventCommand {
        event,
        now_millis: crate::persistence::now_millis(),
    };
    apply(database, owner, vec![AdapterInboundEvent::Session(command)]);
}

/// A hot session on a real database whose only Turn has completed, so the
/// next send is admissible.
fn idle_session() -> IdleSession {
    let root = std::env::temp_dir().join(format!("sage-send-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project_id =
        SageProjectId::parse(database.create_project_from_root(&root).unwrap().id).unwrap();
    let chat_id = SageChatId::parse(
        database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap(),
    )
    .unwrap();
    let epoch = RuntimeEpoch::new(1).unwrap();
    let mut connection = database.connection();
    let initial = projection(&connection, &chat_id);
    let started = start_session(
        &mut connection,
        StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: initial,
            client_intent_id: ClientIntentId::parse("race-start").unwrap(),
            binding_id: BindingId::new("binding", "fixture").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: 1,
        },
    )
    .unwrap();
    activate_session(
        &mut connection,
        ActivateSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::parse("race-activate").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            artifact_digest: "d".repeat(64),
            now_millis: 2,
        },
    )
    .unwrap();
    let external_session_id = ExternalId::new("external", "race-session").unwrap();
    let ready = complete_activation(
        &mut connection,
        CompleteActivationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: external_session_id.clone(),
            capabilities_json: capabilities(),
            now_millis: 3,
        },
    )
    .unwrap();
    let prepared = prepare_turn(
        &mut connection,
        PrepareTurnCommand {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            expected_projection_revision: ready.projection_revision,
            client_intent_id: ClientIntentId::parse("race-first-turn").unwrap(),
            prompt: Prompt::new("prompt", "first").unwrap(),
            now_millis: 4,
        },
    )
    .unwrap();
    begin_send(
        &mut connection,
        TurnMutationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 5,
        },
    )
    .unwrap();
    drop(connection);
    let owner = AdapterStoreOwner {
        project_id,
        chat_id,
        session_id: started.session_id,
        runtime_epoch: epoch,
        turn_id: prepared.turn_id,
        binding_digest: "a".repeat(64),
        external_session_id,
    };
    let turn = |state, now_millis| AdapterInboundEvent::Turn { state, now_millis };
    apply(
        &database,
        &owner,
        vec![turn(TurnState::Running, 6), turn(TurnState::Completed, 7)],
    );
    IdleSession {
        database,
        owner,
        root,
    }
}

fn send(owner: &AdapterStoreOwner, seen: ProjectionRevision, intent: &str) -> PrepareTurnCommand {
    PrepareTurnCommand {
        project_id: owner.project_id.clone(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        expected_projection_revision: seen,
        client_intent_id: ClientIntentId::parse(intent).unwrap(),
        prompt: Prompt::new("prompt", "next").unwrap(),
        now_millis: crate::persistence::now_millis(),
    }
}

#[test]
fn a_racing_send_survives_neutral_session_events_but_not_a_model_change() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let IdleSession {
            database,
            owner,
            root,
        } = idle_session();
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let runner = runner.spawn();
        let coordinator = SendCoordinator::new(
            Arc::new(DatabaseAuthority(database.clone())),
            journal.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
            })
            .unwrap();

        // The effective model moved after the read: the send is stale.
        let seen = projection(&database.connection(), &owner.chat_id);
        commit(
            &database,
            &owner,
            notice(SessionNoticeLevel::Info, "rerouting"),
        );
        commit(
            &database,
            &owner,
            SessionScopedEvent::TurnModel {
                model_id: ModelId::new("modelId", "rerouted").unwrap(),
            },
        );
        let stale = coordinator
            .prepare(&tree, send(&owner, seen, "stale-send"))
            .await
            .map(|_| ());

        // Only a notice, a title and an activity row landed after the read.
        let seen = projection(&database.connection(), &owner.chat_id);
        commit(
            &database,
            &owner,
            notice(SessionNoticeLevel::Warning, "config deprecated"),
        );
        commit(&database, &owner, title("Fix the sidebar"));
        commit(
            &database,
            &owner,
            SessionScopedEvent::Activity(activity("task", SessionActivityState::Running, "1/3")),
        );
        let advanced = projection(&database.connection(), &owner.chat_id) > seen;
        let racing = match coordinator
            .prepare(&tree, send(&owner, seen, "racing-send"))
            .await
        {
            Ok(PrepareDispatch::Applied(dispatch)) => Ok(turn_state(&database, &dispatch)),
            Ok(PrepareDispatch::Duplicate(_)) => Err("duplicate".to_owned()),
            Err(failure) => Err(format!("{:?}", failure.error)),
        };

        // Stop the journal before asserting, so a failure fails, never hangs.
        journal.close_when_drained();
        runner.await.unwrap();
        drop(database);
        std::fs::remove_dir_all(root).unwrap();

        assert!(matches!(
            stale,
            Err(failure) if matches!(failure.error, CoordinatorError::Intent)
        ));
        assert!(advanced);
        assert_eq!(racing, Ok("queued".to_owned()));
    });
}

fn turn_state(database: &Database, dispatch: &ActiveDispatch) -> String {
    database
        .connection()
        .query_row(
            "SELECT state FROM turns WHERE id=?1",
            [dispatch.turn_id().as_str()],
            |row| row.get(0),
        )
        .unwrap()
}
