use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{
        AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
    },
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
    },
    fixture::{adapter::FixtureAdapter, generated_cases::FrozenCase, wire},
    intent::{ExternalId, ProjectionChangedV1, Prompt, SafeResourceRef, UnavailableResourceReason},
    lifecycle::TurnState,
    runtime::{
        NotificationError, NotificationSink, NotificationSinkFuture, ProjectionNotifier,
        PublishOutcome,
    },
    store::{
        AdapterInboundBatchCommand, AdapterInboundEvent, AdapterStoreOwner,
        apply_adapter_inbound_batch,
    },
    transport::{Frame, IncrementalFrameDecoder, validate_structure},
};

pub(crate) const CASE_IDS: &[&str] = &["stale-project-generation-completion"];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    assert_eq!(case.id, CASE_IDS[0]);
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let frame = decode_one(&stdout);
    let mut adapter = ready_adapter();
    let batch = adapter.ingest(&frame, &Mapper).unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        }]
    ));

    let database = durable_database();
    let mut connection = database.connection();
    let mutation = apply_adapter_inbound_batch(
        &mut connection,
        AdapterInboundBatchCommand::new(
            AdapterStoreOwner {
                project_id: SageProjectId::parse("project-a").unwrap(),
                chat_id: SageChatId::parse("chat-a").unwrap(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                turn_id: SageTurnId::parse("turn-a").unwrap(),
                binding_digest: fixture_binding_digest(),
                external_session_id: ExternalId::new("externalSession", "fixture-external-session")
                    .unwrap(),
            },
            batch.actual_bytes(),
            batch.actual_bytes(),
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Completed,
                now_millis: 2,
            }],
        )
        .unwrap(),
    )
    .unwrap();
    let durable_state: String = connection
        .query_row("SELECT state FROM turns WHERE id='turn-a'", [], |row| {
            row.get(0)
        })
        .unwrap();
    drop(connection);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let sink = Arc::new(CountingSink::default());
        let notifier = ProjectionNotifier::start(sink.clone());
        notifier.set_active_project(SageProjectId::parse("project-new").unwrap());
        let outcome = notifier
            .publish_committed(ProjectionChangedV1 {
                project_id: SageProjectId::parse("project-a").unwrap(),
                chat_id: SageChatId::parse("chat-a").unwrap(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                projection_revision: mutation.projection_revision,
            })
            .unwrap();
        tokio::task::yield_now().await;
        let active_projection_changed = sink.entries.load(Ordering::SeqCst) != 0;
        notifier.shutdown().await;
        json!({
            "durableOwningTurnMayComplete": durable_state == "completed"
                && outcome == PublishOutcome::SuppressedInactive,
            "activeForeignProjectProjectionChanged": active_projection_changed,
        })
    })
}

fn ready_adapter() -> FixtureAdapter {
    let mut adapter = FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("session-a").unwrap(),
        ExternalId::new("externalSession", "fixture-external-session").unwrap(),
    )
    .unwrap();
    let egress = adapter
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "read only").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
        })
        .unwrap();
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    adapter
}

fn durable_database() -> crate::persistence::Database {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-a','project-a',1,1,1)",[]).unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-a','project-a','C:/project','c:/project',0)",[]).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat-a','project-a',0,'chat-a',1,1)",[]).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-a','project-a','chat-a',0,'fixture',?1,'fixture-external-session','{}','ready','hot',0,0,0,1,1)",[fixture_binding_digest()]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-a','chat-a','session-a',1,'running',?1,1)",["d".repeat(64)]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-a','project-a','chat-a','session-a',1,0,'turn-a','running','correlated',0,1,1)",[]).unwrap();
    connection
        .execute(
            "INSERT INTO acp_project_usage VALUES('project-a',1,1,1,0,0,0,0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage VALUES('project-a','chat-a','session-a',1,0,0,0,0,0,0)",
            [],
        )
        .unwrap();
    drop(connection);
    database
}

fn fixture_binding_digest() -> String {
    "07".repeat(32)
}

fn decode_one(bytes: &[u8]) -> Frame {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8 * 1024) {
        if let Some(value) = decoder.push_chunk(chunk).unwrap() {
            assert!(frame.replace(value).is_none());
        }
        while decoder.has_pending_input() {
            if let Some(value) = decoder.drain_pending().unwrap() {
                assert!(frame.replace(value).is_none());
            }
        }
    }
    assert!(decoder.finish().unwrap().is_none());
    let frame = frame.unwrap();
    validate_structure(&frame).unwrap();
    frame
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

#[derive(Default)]
struct CountingSink {
    entries: AtomicUsize,
}

impl NotificationSink for CountingSink {
    fn publish<'a>(&'a self, _event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            self.entries.fetch_add(1, Ordering::SeqCst);
            Ok::<(), NotificationError>(())
        })
    }
}
