use std::{future::Future, pin::Pin};

use super::*;
use crate::acp::{
    bindings::{adapter::ResourceMapper, pi::PiRuntime},
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId, SchemaVersionV1,
    },
    intent::{CapabilitySnapshotV1, SafeResourceRef, UnavailableResourceReason},
    runtime::{
        ActivationFlush, ActivationIntent, ActivationJournal, ActivationJournalError,
        ActivationMode, ActivationPump, ActivationReceipt, ActivationTransport,
    },
    store::CompleteActivationCommand,
    transport::{FrameWriteOutcome, OutboundFrame},
};

#[test]
fn pi_fixture_wire_initialization_reaches_ready_through_shared_activation_pump() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let epoch = RuntimeEpoch::new(7).unwrap();
            let session_id = SageAgentSessionId::parse("session-pi-wire").unwrap();
            let adapter = PiRuntime::fixture(epoch, session_id.clone());
            let ceiling = capabilities();
            let mut pump = ActivationPump::new(
                adapter,
                Journal(None),
                NoResources,
                ActivationIntent {
                    binding_digest: [7; 32],
                    artifact_digest: "ab".repeat(32),
                    project_id: SageProjectId::parse("project-pi-wire").unwrap(),
                    chat_id: SageChatId::parse("chat-pi-wire").unwrap(),
                    session_id,
                    runtime_epoch: epoch,
                    mode: ActivationMode::New {
                        native_root: std::env::current_dir().unwrap().to_str().unwrap().into(),
                        model: None,
                    },
                    curated_ceiling: ceiling.clone(),
                    sage_policy: ceiling,
                },
            )
            .unwrap();
            let mut wire = WireState::new();
            let mut transport = Transport(None);
            pump.begin().unwrap();
            for (index, (kind, command)) in [
                ("piInitialize", "get_state"),
                ("piModels", "get_available_models"),
                ("piNew", "new_session"),
            ]
            .into_iter()
            .enumerate()
            {
                assert!(!pump.completion_ready());
                assert_eq!(
                    pump.flush_one(&mut transport).unwrap(),
                    ActivationFlush::Written
                );
                let request = transport.0.take().unwrap();
                let value: Value = serde_json::from_slice(&request).unwrap();
                assert_eq!(value["type"], command);
                assert_eq!(value["id"], format!("pi-7-{}", index + 1));
                let mut response = Vec::new();
                execute_with_io(
                    &json!({"op":"awaitClient","kind":kind}),
                    &mut wire,
                    &mut Cursor::new(request),
                    &mut response,
                    &mut Vec::new(),
                )
                .unwrap();
                let reply: Value = serde_json::from_slice(&response).unwrap();
                assert_eq!(reply["id"], value["id"]);
                assert_eq!(reply["command"], command);
                pump.push_stdout_chunk(&response, index as i64)
                    .await
                    .unwrap();
            }
            assert!(pump.completion_ready());
            assert_eq!(
                pump.flush_one(&mut transport).unwrap(),
                ActivationFlush::Idle
            );
            let parts = pump.try_into_idle_parts().ok().unwrap();
            assert_eq!(
                parts.session().external_session_id.as_str(),
                "pi-session-7-1"
            );
            assert_eq!(parts.session().models.len(), 2);
            assert_eq!(
                parts
                    .session()
                    .models
                    .iter()
                    .filter(|model| model.is_default)
                    .count(),
                1
            );
            assert_eq!(parts.session().capabilities.model_count, 2);
            let committed = parts.journal().0.as_ref().unwrap();
            assert_eq!(
                committed.external_session_id,
                parts.session().external_session_id
            );
        });
}

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

struct Transport(Option<Vec<u8>>);

impl ActivationTransport for Transport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        assert!(self.0.is_none());
        let mut bytes = frame.payload().to_vec();
        bytes.push(b'\n');
        self.0 = Some(bytes);
        FrameWriteOutcome::Complete
    }
}

struct Journal(Option<CompleteActivationCommand>);

impl ActivationJournal for Journal {
    fn commit_ready<'a>(
        &'a mut self,
        command: CompleteActivationCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ActivationReceipt, ActivationJournalError>> + Send + 'a>>
    {
        assert!(self.0.is_none());
        self.0 = Some(command);
        Box::pin(async {
            Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(1).unwrap(),
            })
        })
    }
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "pi-rpc-0.83.0").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 2,
        prompt_resources: false,
        config_count: 2,
    }
}
