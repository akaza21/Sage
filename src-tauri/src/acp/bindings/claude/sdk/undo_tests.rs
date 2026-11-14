use super::{
    launch::SessionLaunch,
    transcript::{self, TranscriptError},
};
use crate::acp::{
    contract::{
        ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SageAgentSessionId,
        SageTurnId,
    },
    intent::{ExternalId, Prompt},
    lifecycle::TurnState,
    store::{self, StoreError, TurnUndoneCommand, turn_undo::fixture::Fixture},
};
use std::{io::Write, path::PathBuf};

const SESSION: &str = "00000000-0000-4000-8000-000000000090";
const FIRST: &str = "turn-00000000-0000-4000-8000-000000000001";
const LAST: &str = "turn-00000000-0000-4000-8000-000000000003";
const REPLY: &str = "00000000-0000-4000-8000-000000000002";
const DIGEST: &str = "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2";

struct Chat {
    fixture: Fixture,
    config: PathBuf,
    transcript: PathBuf,
}

impl Chat {
    fn new(two_turns: bool) -> Self {
        let capabilities = store::turn_undo::fixture::CAPABILITIES.replace("pending", "fixture");
        let fixture = Fixture::with_thread(super::CLAUDE_SDK_BINDING_ID, SESSION, &capabilities);
        fixture.run();
        fixture.turn(FIRST, 0);
        if two_turns {
            fixture.turn(LAST, 1);
        }
        let config = fixture.root.join("profile");
        let project = config.join("projects").join("-w");
        std::fs::create_dir_all(&project).unwrap();
        let transcript = project.join(format!("{SESSION}.jsonl"));
        let entries = (1..=if two_turns { 4 } else { 2 }).map(|n| {
            serde_json::json!({"type":if n % 2 == 1 { "user" } else { "assistant" },
                "uuid":format!("00000000-0000-4000-8000-{n:012}"),
                "parentUuid":if n == 1 { None } else { Some(format!("00000000-0000-4000-8000-{:012}", n-1)) },
                "isSidechain":false}).to_string()
        }).collect::<Vec<_>>().join("\n");
        std::fs::write(&transcript, entries).unwrap();
        Self {
            fixture,
            config,
            transcript,
        }
    }

    /// Uses the same transcript resolver and durable admission as the service;
    /// completion below represents the harness's successful control response.
    fn stage(&self, turn: &str) -> Result<TurnUndoneCommand, String> {
        let owner = crate::acp::scheduler::RuntimeOwner {
            project_id: self.fixture.project.clone(),
            chat_id: self.fixture.chat.clone(),
            session_id: SageAgentSessionId::parse("session-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(self.fixture.epoch() as u64).unwrap(),
        };
        crate::acp::turn_undo::stage_turn_undo(
            &self.fixture.database,
            &owner,
            SageTurnId::parse(turn).unwrap(),
            false,
            || transcript::resolve_undo(&self.config, SESSION, turn),
        )
        .map_err(|error| match error {
            crate::acp::errors::SessionControlError::Refused(Some(reason)) => {
                reason.as_str().to_owned()
            }
            error => error.to_string(),
        })
    }

    fn send(
        &self,
        session: SageAgentSessionId,
        epoch: u64,
    ) -> Result<store::PreparedTurn, StoreError> {
        let revision = self.fixture.revision();
        store::prepare_turn(
            &mut self.fixture.database.connection(),
            store::PrepareTurnCommand {
                project_id: self.fixture.project.clone(),
                project_generation: ProjectGeneration::new(1).unwrap(),
                chat_id: self.fixture.chat.clone(),
                session_id: session,
                runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::new(),
                prompt: Prompt::new("prompt", "Continue without the undone turn").unwrap(),
                now_millis: 100,
            },
        )
    }

    fn activate(&self, session: &SageAgentSessionId, epoch: u64, external: &str) {
        let revision = self.fixture.revision();
        let mut connection = self.fixture.database.connection();
        store::activate_session(
            &mut connection,
            store::ActivateSessionCommand {
                project_id: self.fixture.project.clone(),
                chat_id: self.fixture.chat.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session.clone(),
                runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
                artifact_digest: DIGEST.into(),
                now_millis: 80,
            },
        )
        .unwrap();
        store::complete_activation(
            &mut connection,
            store::CompleteActivationCommand {
                project_id: self.fixture.project.clone(),
                chat_id: self.fixture.chat.clone(),
                session_id: session.clone(),
                runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
                binding_digest: DIGEST.into(),
                artifact_digest: DIGEST.into(),
                external_session_id: ExternalId::new("externalSessionId", external).unwrap(),
                capabilities_json: store::turn_undo::fixture::CAPABILITIES
                    .replace("pending", "fixture"),
                now_millis: 90,
            },
        )
        .unwrap();
    }
}

#[test]
fn undo_after_duplicated_history_cuts_at_the_previous_reply() {
    let chat = Chat::new(true);
    let saved = std::fs::read_to_string(&chat.transcript).unwrap();
    let history = saved.lines().take(2).collect::<Vec<_>>().join("\n");
    std::fs::write(&chat.transcript, format!("{history}\n{saved}")).unwrap();

    let command = chat.stage(LAST).unwrap();
    assert_eq!(command.cut_at.as_deref(), Some(REPLY));
    let recorded =
        store::record_turn_undone(&mut chat.fixture.database.connection(), &command).unwrap();
    assert_eq!(recorded.thread, store::UndoneThread::Cut);
    assert_eq!(chat.fixture.page_turns(), [FIRST]);
}

#[test]
fn undo_on_conflicting_duplicate_history_fails_and_keeps_the_turn() {
    for (field, value) in [
        ("parentUuid", serde_json::Value::Null),
        ("type", serde_json::json!("user")),
        ("isSidechain", serde_json::json!(true)),
    ] {
        let chat = Chat::new(true);
        let saved = std::fs::read_to_string(&chat.transcript).unwrap();
        let mut duplicate: serde_json::Value =
            serde_json::from_str(saved.lines().nth(1).unwrap()).unwrap();
        duplicate[field] = value;
        std::fs::write(&chat.transcript, format!("{duplicate}\n{saved}")).unwrap();

        assert_eq!(
            chat.stage(LAST).unwrap_err(),
            TranscriptError::ParentChain.to_string()
        );
        assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
        assert!(chat.fixture.pending().is_none());
    }
}

#[test]
fn undo_on_an_unreadable_transcript_fails_and_keeps_the_turn() {
    let chat = Chat::new(true);
    std::fs::remove_file(&chat.transcript).unwrap();
    assert_eq!(
        chat.stage(LAST).unwrap_err(),
        TranscriptError::Unreadable.to_string()
    );
    assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
    assert!(chat.fixture.pending().is_none());
    std::fs::write(
        &chat.transcript,
        serde_json::json!({"type":"user",
        "uuid":"00000000-0000-4000-8000-000000000003"})
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        chat.stage(LAST).unwrap_err(),
        TranscriptError::ParentChain.to_string()
    );
    assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
}

#[test]
fn undo_on_an_oversized_transcript_fails_and_keeps_the_turn() {
    let chat = Chat::new(true);
    std::fs::write(&chat.transcript, vec![b'x'; transcript::MAX_LINE_BYTES + 1]).unwrap();
    assert_eq!(
        chat.stage(LAST).unwrap_err(),
        TranscriptError::LineLimit.to_string()
    );
    assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
    std::fs::File::create(&chat.transcript)
        .unwrap()
        .set_len(transcript::MAX_TOTAL_BYTES + 1)
        .unwrap();
    assert_eq!(
        chat.stage(LAST).unwrap_err(),
        TranscriptError::ByteLimit.to_string()
    );
    assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
    for entry in [
        "not JSON\n",
        "{}\n",
        "{\"uuid\":\"00000000-0000-4000-8000-000000000009\"}\n",
        "{\"type\":\"assistant\",\"uuid\":\"00000000-0000-4000-8000-000000000009\",\"parentUuid\":null}\n",
    ] {
        let mut file = std::io::BufWriter::new(std::fs::File::create(&chat.transcript).unwrap());
        for _ in 0..=transcript::MAX_ENTRIES {
            file.write_all(entry.as_bytes()).unwrap();
        }
        file.flush().unwrap();
        assert_eq!(
            chat.stage(LAST).unwrap_err(),
            TranscriptError::EntryLimit.to_string()
        );
        assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
    }
}

#[test]
fn undo_of_the_first_turn_allows_an_immediate_send() {
    let chat = Chat::new(false);
    let command = chat.stage(FIRST).unwrap();
    let recorded =
        store::record_turn_undone(&mut chat.fixture.database.connection(), &command).unwrap();
    assert_eq!(recorded.thread, store::UndoneThread::Restart);
    assert!(chat.fixture.page_turns().is_empty());
    store::park_session_after_zero_process(
        &mut chat.fixture.database.connection(),
        store::ParkSessionCommand {
            project_id: chat.fixture.project.clone(),
            chat_id: chat.fixture.chat.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
            exit_kind: RequiredText::new("exitKind", "undo").unwrap(),
            now_millis: 60,
        },
    )
    .unwrap();
    let revision = chat.fixture.revision();
    let restarted = store::restart_undone_session(
        &mut chat.fixture.database.connection(),
        store::RestartSessionCommand {
            project_id: chat.fixture.project.clone(),
            chat_id: chat.fixture.chat.clone(),
            expected_projection_revision: revision,
            session_id: command.session_id,
            now_millis: 70,
        },
    )
    .unwrap()
    .session_id;
    chat.activate(&restarted, 1, "new-empty-thread");
    let prepared = chat.send(restarted.clone(), 1).unwrap();
    let sending = store::begin_send(
        &mut chat.fixture.database.connection(),
        store::TurnMutationCommand {
            project_id: chat.fixture.project.clone(),
            chat_id: chat.fixture.chat.clone(),
            session_id: restarted,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 101,
        },
    )
    .unwrap();
    assert_eq!(sending.state, TurnState::Sending);
    assert_eq!(chat.fixture.page_turns(), [prepared.turn_id.as_str()]);
}

#[path = "undo_recovery_tests.rs"]
mod recovery;
