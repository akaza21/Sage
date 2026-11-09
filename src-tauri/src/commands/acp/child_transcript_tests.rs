use super::*;
use crate::acp::bindings::codex::CODEX_OBSERVED_PROTOCOL_IDENTITY;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
use crate::acp::bindings::codex::CODEX_PINNED_VERSION;
use sha2::Digest;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use crate::{
    acp::{
        bindings::codex::{NativeTurnsPage, test_transcript::child_read as child_read_for_test},
        contract::{ClientIntentId, RequiredText, SafeText, SageTurnId, SchemaVersionV1},
        goal::{GoalState, NativeRead, NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
        intent::{BindingId, CapabilitySnapshotV1},
        runtime::{
            HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
            NativeRpcCompletionSink, PromptWrite, RuntimeEngine, RuntimeHost,
        },
        store::{
            ActivateSessionCommand, AdapterTurnCursor, CompleteActivationCommand,
            StartSessionCommand, activate_session, complete_activation, start_session,
        },
    },
    persistence::Database,
};

struct ChildEngine {
    snapshot: ChildTranscriptSnapshot,
    replies: VecDeque<NativeRpcReply>,
    calls: Arc<AtomicUsize>,
}

impl RuntimeEngine for ChildEngine {
    fn begin_native_rpc(
        &mut self,
        intent: &NativeRpcIntent,
        _: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        let NativeRpcRequest::Read(NativeRead::ChildTurns {
            child_thread_id,
            parent_thread_id,
            cursor: None,
        }) = &intent.request
        else {
            return Err(HostError::Intent);
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        if *child_thread_id != self.snapshot.child_thread_id
            || *parent_thread_id != self.snapshot.parent_thread_id
        {
            return Err(HostError::Intent);
        }
        completion.complete(Ok(self.replies.pop_front().unwrap_or_else(|| {
            NativeRpcReply::TurnsPage(NativeTurnsPage {
                turns: self.snapshot.turns.clone(),
                next_cursor: None,
            })
        })));
        Ok(())
    }
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
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
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

#[tokio::test]
async fn native_child_ipc_validates_binding_and_returns_persisted_typed_reply() {
    child_ipc_journey(
        ProjectGeneration::ZERO,
        ProjectGeneration::ZERO,
        true,
        CODEX_OBSERVED_PROTOCOL_IDENTITY,
    )
    .await;
}

/// The native capture's desktop fixture replays the pinned `thread/read` wire
/// under its own identity; its child view reads that replay (wave 9 C1).
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[tokio::test]
async fn native_child_ipc_reads_the_desktop_fixture_replay() {
    child_ipc_journey(
        ProjectGeneration::ZERO,
        ProjectGeneration::ZERO,
        false,
        &format!("codex-app-server-desktop-fixture-{CODEX_PINNED_VERSION}"),
    )
    .await;
}

#[tokio::test]
async fn native_child_ipc_rejects_drifted_projection_generation_but_accepts_live_generation() {
    child_ipc_journey(
        ProjectGeneration::new(3).unwrap(),
        ProjectGeneration::new(2).unwrap(),
        false,
        CODEX_OBSERVED_PROTOCOL_IDENTITY,
    )
    .await;
}

async fn child_ipc_journey(
    projection_generation: ProjectGeneration,
    native_generation: ProjectGeneration,
    export_fixture: bool,
    protocol: &str,
) {
    let export_path = std::env::var_os("SAGE_CP18D_EXPORT_DB")
        .filter(|_| export_fixture)
        .map(std::path::PathBuf::from);
    let root = std::env::var_os("SAGE_CP18D_PROJECT_ROOT")
        .filter(|_| export_fixture)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("sage-cp18d-ipc-{}", uuid::Uuid::new_v4()))
        });
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let state = AppState::new(database);
    *state.project_transitions.lock().await = native_generation.get();
    let project_id = SageProjectId::parse(&project.id).unwrap();
    let chat_id = SageChatId::parse(&chat).unwrap();
    let revision = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let session = {
        let mut connection = state.database.connection();
        start_session(
            &mut connection,
            StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("child-start").unwrap(),
                binding_id: BindingId::new("binding", "sage-simulator").unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
        )
        .unwrap()
    };
    let epoch = RuntimeEpoch::new(1).unwrap();
    let revision = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    {
        let mut connection = state.database.connection();
        activate_session(
            &mut connection,
            ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("child-activate").unwrap(),
                session_id: session.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: "a".repeat(64),
                now_millis: 2,
            },
        )
        .unwrap();
        let capabilities = CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocol", protocol).unwrap(),
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
            child_transcript_read: true,
            collaboration_event_projection: false,
            multi_root_extension_schema_id: None,
            model_count: 1,
            prompt_resources: false,
            prompt_images: false,
            config_count: 0,
        };
        complete_activation(
            &mut connection,
            CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: "a".repeat(64),
                artifact_digest: "a".repeat(64),
                external_session_id: ExternalId::new("session", "adapter-session").unwrap(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: 3,
            },
        )
        .unwrap();
        connection.execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one',?1,?2,?3,1,0,'intent-one','running','correlated',0,1,1)",
            rusqlite::params![project_id.as_str(), chat_id.as_str(), session.session_id.as_str()],
        ).unwrap();
        let payload = crate::acp::intent::NormalizedPayload::Collaboration {
            label: SafeText::new("label", "Worker").unwrap(),
            message: None,
            receiver_labels: Default::default(),
            operation: Some(SafeText::new("operation", "started").unwrap()),
            agent_key: None,
            agent_keys: Default::default(),
            agent_states: Default::default(),
            model: None,
            reasoning_effort: None,
        };
        let encoded = serde_json::to_string(&payload).unwrap();
        let digest = format!("{:x}", sha2::Sha256::digest(encoded.as_bytes()));
        connection.execute(
            "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item-one',?1,?2,?3,'turn-one',1,0,'primary',NULL,'collaboration','started',NULL,'activity-one',?4,?5,?6,0,1,1,NULL)",
            rusqlite::params![project_id.as_str(), chat_id.as_str(), session.session_id.as_str(), encoded, encoded.len() as i64, digest],
        ).unwrap();
        connection.execute("INSERT INTO acp_child_thread_owners(native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,parent_thread_id,source,created_at) VALUES('child-one',?1,?2,?3,1,'adapter-session','codexSubAgentActivity',1)",
            rusqlite::params![project_id.as_str(), chat_id.as_str(), session.session_id.as_str()]).unwrap();
        connection.execute("INSERT INTO acp_child_thread_bindings(item_id,project_id,chat_id,session_id,turn_id,runtime_epoch,native_child_thread_id,created_at) VALUES('item-one',?1,?2,?3,'turn-one',1,'child-one',1)",
            rusqlite::params![project_id.as_str(), chat_id.as_str(), session.session_id.as_str()]).unwrap();
        connection.execute("UPDATE acp_session_usage SET turn_count=turn_count+1,item_count=item_count+1,item_bytes=item_bytes+?2,payload_bytes=payload_bytes+?2 WHERE session_id=?1",
            rusqlite::params![session.session_id.as_str(), encoded.len() as i64]).unwrap();
        connection.execute("UPDATE acp_project_usage SET turn_count=turn_count+1,item_count=item_count+1,payload_bytes=payload_bytes+?2 WHERE project_id=?1",
            rusqlite::params![project_id.as_str(), encoded.len() as i64]).unwrap();
        connection.execute("UPDATE acp_store_usage SET turn_count=turn_count+1,item_count=item_count+1,item_bytes=item_bytes+?1,shared_payload_bytes=shared_payload_bytes+?1 WHERE singleton=1",
            [encoded.len() as i64]).unwrap();
        // Child reads require a completed parent Turn while its runtime epoch remains running.
        connection
            .execute(
                "UPDATE turns SET state='completed',terminal_at=4 WHERE id='turn-one'",
                [],
            )
            .unwrap();
    }
    let wire = include_bytes!("../../acp/runtime/adapter_pump/fixtures/cp18d_child_read.json");
    let mapper = crate::acp::resource_mapper::NativeResourceMapper::new([]).unwrap();
    let snapshot = child_read_for_test(wire, 17, "child-one", "adapter-session", &mapper).unwrap();
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session.session_id.clone(),
        runtime_epoch: epoch,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        RuntimeHost::start(ChildEngine {
            snapshot: snapshot.clone(),
            replies: VecDeque::from([NativeRpcReply::Rejected, NativeRpcReply::TooLarge]),
            calls: Arc::clone(&calls),
        })
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    state
        .acp
        .install_goal_test_runtime(owner, host, GoalState::Unsupported);
    let mut request = ChildTranscriptRequest {
        project_id,
        project_generation: projection_generation,
        chat_id,
        session_id: session.session_id,
        runtime_epoch: epoch,
        item_id: SageItemId::parse("item-one").unwrap(),
        agent_key: ExternalId::new("child", "child-one").unwrap(),
    };
    if projection_generation != native_generation {
        assert!(matches!(
            get_acp_child_transcript_inner(&state, request.clone()).await,
            Err(AcpIpcError::Conflict {
                resource: crate::acp::intent::AcpConflictResource::ProjectGeneration,
                ..
            })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        request.project_generation = native_generation;
    }
    let scope = crate::acp::store::child_transcript::ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: request.project_id.clone(),
            chat_id: request.chat_id.clone(),
            session_id: request.session_id.clone(),
            runtime_epoch: request.runtime_epoch,
        },
        item_id: request.item_id.as_str().to_owned(),
        child_thread_id: request.agent_key.as_str().to_owned(),
        parent_thread_id: "adapter-session".into(),
    };
    assert!(
        crate::acp::store::child_transcript::verify_current_binding(
            &state.database.connection(),
            &scope
        )
        .is_ok()
    );
    let mut foreign = request.clone();
    foreign.agent_key = ExternalId::new("child", "other-child").unwrap();
    assert!(
        get_acp_child_transcript_inner(&state, foreign)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // A child read carries no projection revision: steady parent activity
    // never makes it stale (B2), and the read never moves the revision.
    let mut foreign_session = request.clone();
    foreign_session.session_id = SageAgentSessionId::parse("foreign-session").unwrap();
    assert!(matches!(
        get_acp_child_transcript_inner(&state, foreign_session).await,
        Err(AcpIpcError::Conflict {
            resource: crate::acp::intent::AcpConflictResource::Session,
            ..
        })
    ));
    let mut foreign_epoch = request.clone();
    foreign_epoch.runtime_epoch = RuntimeEpoch::new(2).unwrap();
    assert!(matches!(
        get_acp_child_transcript_inner(&state, foreign_epoch).await,
        Err(AcpIpcError::Conflict {
            resource: crate::acp::intent::AcpConflictResource::RuntimeEpoch,
            ..
        })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let result = get_acp_child_transcript_inner(&state, request.clone()).await;
    assert!(matches!(result, Err(AcpIpcError::Rejected { .. })));
    assert!(matches!(
        get_acp_child_transcript_inner(&state, request.clone()).await,
        Ok(ChildTranscriptResponse::TooLarge)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let stored: i64 = state
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM acp_child_thread_transcripts",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, 0);
    let result = get_acp_child_transcript_inner(&state, request.clone())
        .await
        .unwrap();
    assert!(
        matches!(result, ChildTranscriptResponse::Current { snapshot: stored, spawn_model: None } if stored == snapshot)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let count: i64 = state.database.connection().query_row("SELECT COUNT(*) FROM acp_child_thread_transcripts WHERE native_child_thread_id='child-one'", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 1);
    if projection_generation != native_generation {
        *state.project_transitions.lock().await = 4;
        assert!(matches!(
            get_acp_child_transcript_inner(&state, request.clone()).await,
            Err(AcpIpcError::Conflict {
                resource: crate::acp::intent::AcpConflictResource::ProjectGeneration,
                ..
            })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        request.project_generation = ProjectGeneration::new(4).unwrap();
        assert!(matches!(
            get_acp_child_transcript_inner(&state, request.clone()).await,
            Ok(ChildTranscriptResponse::Current { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 4);
    }
    let mut foreign_owner = scope.clone();
    foreign_owner.owner.session_id = SageAgentSessionId::parse("foreign-session").unwrap();
    assert!(
        crate::acp::store::child_transcript::load_earlier_snapshot(
            &state.database.connection(),
            &foreign_owner,
        )
        .unwrap()
        .is_none()
    );
    {
        let connection = state.database.connection();
        connection.execute("UPDATE session_runtime_epochs SET state='terminating' WHERE session_id=?1 AND runtime_epoch=1", [request.session_id.as_str()]).unwrap();
        assert!(
            crate::acp::store::child_transcript::verify_current_binding(&connection, &scope)
                .is_err()
        );
        assert!(
            crate::acp::store::child_transcript::load_earlier_snapshot(&connection, &scope)
                .unwrap()
                .is_none()
        );
        connection.execute("UPDATE session_runtime_epochs SET state='exited',terminal_at=12,exit_kind='processExit' WHERE session_id=?1 AND runtime_epoch=1", [request.session_id.as_str()]).unwrap();
        assert_eq!(
            crate::acp::store::child_transcript::load_earlier_snapshot(&connection, &scope)
                .unwrap(),
            Some(snapshot)
        );
        if let Some(path) = export_path.as_ref() {
            let mut export = rusqlite::Connection::open(path).unwrap();
            rusqlite::backup::Backup::new(&connection, &mut export)
                .unwrap()
                .run_to_completion(256, std::time::Duration::ZERO, None)
                .unwrap();
        }
        connection
            .execute(
                "DELETE FROM chat_session_heads WHERE session_id=?1",
                [request.session_id.as_str()],
            )
            .unwrap();
        assert!(
            crate::acp::store::child_transcript::load_earlier_snapshot(&connection, &scope)
                .unwrap()
                .is_none()
        );
        connection.execute("UPDATE agent_sessions SET state='failed',residency='cold',terminal_at=13 WHERE id=?1", [request.session_id.as_str()]).unwrap();
        assert!(
            crate::acp::store::child_transcript::load_earlier_snapshot(&connection, &scope)
                .unwrap()
                .is_none()
        );
    }
    state.acp.shutdown_blocking().unwrap();
    if export_path.is_none() {
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn child_reply_names_a_spawn_model_only_when_one_was_supplied() {
    let snapshot = ChildTranscriptSnapshot {
        child_thread_id: "child-one".into(),
        parent_thread_id: "parent".into(),
        turns: Vec::new(),
        retention_limited: false,
    };
    let with = serde_json::to_value(ChildTranscriptResponse::Current {
        snapshot: snapshot.clone(),
        spawn_model: Some(SpawnModel {
            model: "gpt-6-astra".into(),
            reasoning_effort: Some("medium".into()),
        }),
    })
    .unwrap();
    assert_eq!(
        with["spawnModel"],
        serde_json::json!({"model": "gpt-6-astra", "reasoningEffort": "medium"})
    );
    let without = serde_json::to_value(ChildTranscriptResponse::FromEarlierSession {
        snapshot,
        spawn_model: None,
    })
    .unwrap();
    assert_eq!(without["kind"], "fromEarlierSession");
    assert!(without.get("spawnModel").is_none());
}
