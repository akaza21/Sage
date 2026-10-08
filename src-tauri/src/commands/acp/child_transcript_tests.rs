use super::*;
use sha2::Digest;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use crate::{
    acp::{
        bindings::codex::normalize_read_response,
        contract::{ClientIntentId, RequiredText, SafeText, SageTurnId, SchemaVersionV1},
        goal::{GoalState, NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
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
    calls: Arc<AtomicUsize>,
}

impl RuntimeEngine for ChildEngine {
    fn begin_native_rpc(
        &mut self,
        intent: &NativeRpcIntent,
        _: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        let NativeRpcRequest::ChildThreadRead {
            child_thread_id,
            parent_thread_id,
        } = &intent.request
        else {
            return Err(HostError::Intent);
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        if *child_thread_id != self.snapshot.child_thread_id
            || *parent_thread_id != self.snapshot.parent_thread_id
        {
            return Err(HostError::Intent);
        }
        completion.complete(Ok(NativeRpcReply::ChildTranscript(self.snapshot.clone())));
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
    let root = std::env::temp_dir().join(format!("sage-cp18d-ipc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let state = AppState::new(database);
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
                binding_id: BindingId::new("binding", "child-test").unwrap(),
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
            protocol_version: RequiredText::new("protocol", "codex-app-server-observed-0.146.0")
                .unwrap(),
            new_session: true,
            resume_session: false,
            load_session: false,
            close_session: true,
            prompt: true,
            cancel: true,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: false,
            child_transcript_read: true,
            collaboration_event_projection: false,
            multi_root_extension_schema_id: None,
            model_count: 1,
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
    }
    let wire = include_bytes!("../../acp/runtime/adapter_pump/fixtures/cp18d_child_read_0146.json");
    let mapper = crate::acp::resource_mapper::NativeResourceMapper::new([]).unwrap();
    let snapshot =
        normalize_read_response(wire, 17, "child-one", "adapter-session", &mapper).unwrap();
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
            calls: Arc::clone(&calls),
        })
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    state
        .acp
        .install_goal_test_runtime(owner, host, GoalState::Unsupported);
    let revision = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let request = ChildTranscriptRequest {
        project_id,
        project_generation: ProjectGeneration::ZERO,
        chat_id,
        expected_projection_revision: revision,
        session_id: session.session_id,
        runtime_epoch: epoch,
        item_id: SageItemId::parse("item-one").unwrap(),
        child_thread_id: ExternalId::new("child", "child-one").unwrap(),
        parent_thread_id: ExternalId::new("parent", "adapter-session").unwrap(),
    };
    let scope = crate::acp::store::child_transcript::ChildTranscriptScope {
        owner: RuntimeOwner {
            project_id: request.project_id.clone(),
            chat_id: request.chat_id.clone(),
            session_id: request.session_id.clone(),
            runtime_epoch: request.runtime_epoch,
        },
        item_id: request.item_id.as_str().to_owned(),
        child_thread_id: request.child_thread_id.as_str().to_owned(),
        parent_thread_id: request.parent_thread_id.as_str().to_owned(),
    };
    assert!(
        crate::acp::store::child_transcript::verify_current_binding(
            &state.database.connection(),
            &scope
        )
        .is_ok()
    );
    let mut foreign = request.clone();
    foreign.parent_thread_id = ExternalId::new("parent", "other-parent").unwrap();
    assert!(
        get_acp_child_transcript_inner(&state, foreign)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let result = get_acp_child_transcript_inner(&state, request)
        .await
        .unwrap();
    assert!(
        matches!(result, ChildTranscriptResponse::Current { snapshot: stored } if stored == snapshot)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let count: i64 = state.database.connection().query_row("SELECT COUNT(*) FROM acp_child_thread_transcripts WHERE native_child_thread_id='child-one'", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 1);
    state.acp.shutdown_blocking().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
