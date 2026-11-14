use std::{
    io::{self, Write},
    sync::Arc,
};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::{AdapterCommand, EgressResolution, HarnessAdapter},
        contract::{
            ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SageChatId,
            SageProjectId, SchemaVersionV1,
        },
        fixture::{
            adapter::FixtureAdapter,
            generated_cases::FrozenCase,
            test_support::{FixtureDeadline, JournalCleanup},
        },
        intent::{CapabilitySnapshotV1, ExternalId, IntentSnapshot, ProjectionChangedV1, Prompt},
        lifecycle::DeliveryState,
        recovery::recover_after_restart,
        runtime::{
            JournalHandle, PrepareDispatch, ProjectIntentAuthority, ProjectionPublishError,
            ProjectionPublishStatus, ProjectionPublisher, SendCoordinator, SessionCoordinator,
        },
        scheduler::Scheduler,
        store::{
            ActivateSessionCommand, CompleteActivationCommand, DatabaseAcpStore,
            PrepareTurnCommand, SendObservation, StartSessionCommand, load_chat_page,
        },
        transport::FrameWriteOutcome,
    },
    persistence::Database,
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &["send-partial-write-failure"];

fn execute(case: &FrozenCase) -> Value {
    match case.id {
        "send-partial-write-failure" => observe_partial_write(case),
        id => panic!("unexpected delivery observer case {id}"),
    }
}

fn observe_partial_write(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [failure, send] = steps.as_slice() else {
        panic!("expected injected write failure followed by one prompt");
    };
    assert_eq!(failure["op"], "injectStdinFailure");
    assert_eq!(send["op"], "sendPrompt");
    let after_bytes = field_u64(failure, "afterBytes") as usize;
    let prompt_bytes = field_u64(send, "bytes") as usize;

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let deadline = FixtureDeadline::new();
        let root = std::env::temp_dir().join(format!("sage-partial-write-{}", Uuid::new_v4()));
        let project_root = root.join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let database_path = root.join("sage.db");
        let database = Database::open(&database_path).unwrap();
        let project = database.create_project_from_root(&project_root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("fixture-chat").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Fixture',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();

        let publisher: Arc<dyn ProjectionPublisher> = Arc::new(Publisher);
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let journal_cleanup = JournalCleanup::spawn(deadline, journal.clone(), runner).unwrap();
        let scheduler = Scheduler::new();
        let sessions = SessionCoordinator::new(journal.clone(), scheduler, publisher.clone());
        let initial = load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let started = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            sessions.start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("fixture-start").unwrap(),
                binding_id: RequiredText::new("binding", "fixture-binding").unwrap(),
                binding_digest: "07".repeat(32),
                harness_version: None,
                model_id: None,
                now_millis: now,
            }),
        )
            .await
            .expect("partial-write fixture session start timed out")
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let mut lease = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            sessions.activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("fixture-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: "08".repeat(32),
                now_millis: now + 1,
            }),
        )
            .await
            .expect("partial-write fixture activation timed out")
            .unwrap();
        let external_session = ExternalId::new("external", "fixture-external").unwrap();
        let ready = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            lease.complete_activation(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: "07".repeat(32),
                artifact_digest: "08".repeat(32),
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 2,
            }),
        )
            .await
            .expect("partial-write fixture activation commit timed out")
            .unwrap();
        let authority = Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            intent_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        }));
        let sends = SendCoordinator::new(authority, journal.clone(), publisher);
        let prompt = Prompt::new("prompt", "p".repeat(prompt_bytes)).unwrap();
        let prepared = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            lease.prepare_turn(
                &sends,
                PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready.projection_revision,
                    client_intent_id: ClientIntentId::parse("fixture-turn").unwrap(),
                    prompt: prompt.clone(),
                    now_millis: now + 3,
                },
            ),
        )
            .await
            .expect("partial-write fixture turn preparation timed out")
            .unwrap();
        let PrepareDispatch::Applied(mut dispatch) = prepared else {
            panic!("fixture prompt must create one Turn");
        };
        tokio::time::timeout(
            deadline.operation_before_cleanup(),
            dispatch.begin_send(now + 4),
        )
        .await
        .expect("partial-write fixture begin-send timed out")
        .unwrap();

        let mut adapter = FixtureAdapter::new(
            [7; 32],
            epoch,
            started.session_id.clone(),
            external_session,
        )
        .unwrap();
        let egress = adapter
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new(
                    "clientMessageId",
                    dispatch.turn_id().as_str().to_owned(),
                )
                .unwrap(),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        adapter
            .resolve_egress(egress.token().clone(), EgressResolution::Queued)
            .unwrap();
        let outcome = write_fixture_frame(&mut InjectedFailureWriter::new(after_bytes), egress.frame());
        let FrameWriteOutcome::DeliveryUnknown { bytes_written } = outcome else {
            panic!("injected positive-byte failure must make delivery unknown");
        };
        assert_eq!(bytes_written, after_bytes);
        adapter
            .resolve_egress(egress.into_token(), EgressResolution::DeliveryUnknown)
            .unwrap();
        let terminal = tokio::time::timeout(
            deadline.operation_before_cleanup(),
            dispatch.observe(SendObservation::PossibleBytes, None, now + 5),
        )
            .await
            .expect("partial-write fixture observation timed out")
            .unwrap();
        assert_eq!(terminal.delivery_state, DeliveryState::PossibleBytes);

        let before = send_started(&database, started.session_id.as_str());
        drop(dispatch);
        drop(lease);
        drop(sends);
        drop(sessions);
        journal_cleanup.finish().unwrap();
        drop(journal);
        drop(database);

        let reopened = Database::open(&database_path).unwrap();
        recover_after_restart(&mut reopened.connection(), now + 6).unwrap();
        let after = send_started(&reopened, started.session_id.as_str());
        let (state, delivery): (String, String) = reopened
            .connection()
            .query_row(
                "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(delivery, "possibleBytes");
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();

        json!({
            "turn": state,
            "automaticReplay": after > before,
        })
    })
}

fn write_fixture_frame(
    output: &mut InjectedFailureWriter,
    frame: &crate::acp::transport::OutboundFrame,
) -> FrameWriteOutcome {
    let mut written = 0usize;
    for bytes in [frame.payload(), b"\n".as_slice()] {
        let mut offset = 0usize;
        while offset < bytes.len() {
            match output.write(&bytes[offset..]) {
                Ok(0) | Err(_) if written == 0 => return FrameWriteOutcome::ZeroByteFailure,
                Ok(0) | Err(_) => {
                    return FrameWriteOutcome::DeliveryUnknown {
                        bytes_written: written,
                    };
                }
                Ok(count) => {
                    written += count;
                    offset += count;
                }
            }
        }
    }
    if output.flush().is_err() {
        return FrameWriteOutcome::DeliveryUnknown {
            bytes_written: written,
        };
    }
    FrameWriteOutcome::Complete
}

struct InjectedFailureWriter {
    remaining: usize,
}

impl InjectedFailureWriter {
    fn new(after_bytes: usize) -> Self {
        Self {
            remaining: after_bytes,
        }
    }
}

impl Write for InjectedFailureWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("fixture stdin failure"));
        }
        let written = bytes.len().min(self.remaining);
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Authority(IntentSnapshot);

impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _command: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

struct Publisher;

impl ProjectionPublisher for Publisher {
    fn publish_committed(
        &self,
        _event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        Ok(ProjectionPublishStatus::Published)
    }
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "fixture-1").unwrap(),
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
        native_background_terminals: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    }
}

fn send_started(database: &Database, session_id: &str) -> i64 {
    database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sendStarted'",
            [session_id],
            |row| row.get(0),
        )
        .unwrap()
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}

#[test]
fn corrected_host_delivery_cases_match_their_literal_oracles() {
    for case_id in super::DELIVERY_CASE_IDS {
        let case = super::super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == *case_id)
            .unwrap();
        super::super::oracle::assert_literal(case, execute(case));
    }
}
