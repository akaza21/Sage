use crate::acp::{
    bindings::claude::sdk::CLAUDE_SDK_BINDING_ID, service::AcpService,
    store::turn_undo::fixture::Fixture,
};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageTurnId},
    scheduler::RuntimeOwner,
};

/// The service starts first: its startup recovery would park a runtime it does
/// not run.
fn running(binding: &str) -> (Fixture, AcpService) {
    let mut caps: sage_acp_protocol::intent::CapabilitySnapshotV1 =
        serde_json::from_str(crate::acp::store::turn_undo::fixture::CAPABILITIES).unwrap();
    caps.rewind = crate::acp::bindings::claude::sdk::rewind_capability("2.1.326");
    if binding == "checkpoint-fixture" {
        caps.rewind.effects.files = false;
        caps.rewind.targets = sage_acp_protocol::intent::RewindTargets::LatestTurn;
        caps.rewind.on_resume = false;
        caps.rewind.runtime_checkpoint = true;
    }
    let fixture = Fixture::with_thread(binding, "thread-a", &serde_json::to_string(&caps).unwrap());
    let service = AcpService::new(fixture.database.clone()).unwrap();
    fixture.run();
    (fixture, service)
}

/// The turn the page offers to undo, and whether the control would be let
/// through for `turn`.
fn offer((fixture, service): &(Fixture, AcpService), turn: &str) -> (Option<String>, bool) {
    let page = composed_page(fixture, service);
    let offered = page
        .session_surface
        .and_then(|surface| surface.undo_turn)
        .map(|turn| turn.as_str().to_owned());
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a".to_owned()).unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let allowed = service
        .undoable_turn(&owner, &SageTurnId::parse(turn.to_owned()).unwrap())
        .unwrap()
        .is_some();
    (offered, allowed)
}

fn composed_page(fixture: &Fixture, service: &AcpService) -> crate::acp::projection::AcpChatPage {
    let mut page = crate::acp::store::load_chat_page(
        &fixture.database.connection(),
        fixture.project.clone(),
        fixture.chat.clone(),
        None,
    )
    .unwrap();
    service.compose_session_surface(&mut page).unwrap();
    page
}

#[test]
fn fork_lineage_does_not_withhold_a_capable_conversation_rewind() {
    let claude = running(CLAUDE_SDK_BINDING_ID);
    let fixture = &claude.0;
    fixture.turn("turn-1", 0);
    assert_eq!(offer(&claude, "turn-1"), (Some("turn-1".into()), true));
    fixture
        .database
        .connection()
        .execute_batch(&format!(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
               VALUES('chat-fork','{project}',9,'Fork',1,1);
             INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,
               parent_chat_id,source_session_id,source_turn_id,relation,external_operation_id,
               created_at)
             VALUES('{project}','chat-fork','{project}','{chat}','session-a','turn-1',
               'nativeFork',NULL,1);",
            project = fixture.project.as_str(),
            chat = fixture.chat.as_str(),
        ))
        .unwrap();
    assert_eq!(offer(&claude, "turn-1"), (Some("turn-1".into()), true));
}

#[test]
fn a_pre_upgrade_claude_turn_with_an_attachment_keeps_its_uuid_checkpoint_undo() {
    use crate::acp::{
        bindings::claude::sdk,
        store::{self, rewind_tests::history},
    };
    let (fixture, service) = running(CLAUDE_SDK_BINDING_ID);
    let turn = SageTurnId::parse("turn-00000000-0000-4000-8000-000000000003").unwrap();
    fixture.turn(turn.as_str(), 0);
    // This is the immutable pre-upgrade representation: attachment metadata,
    // no send-time nativePromptDigest and no nativeTurnTarget fact.
    let prompt = history::file_prompt();
    let payload = serde_json::json!({"kind":"text","payload":prompt.user_payload()}).to_string();
    let epoch = fixture.epoch();
    fixture.database.connection().execute(
        "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
         origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at)
         VALUES('legacy-prompt',?1,?2,'session-a',?3,?4,0,'primary','text','completed',?5,?6,?7,0,1,1,1)",
        rusqlite::params![fixture.project.as_str(), fixture.chat.as_str(), turn.as_str(),
            epoch, payload, payload.len() as i64, format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(payload.as_bytes()))],
    ).unwrap();
    let session = "00000000-0000-4000-8000-000000000090";
    let config = fixture.root.join("profile");
    let directory = config.join("projects").join("project");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(format!("{session}.jsonl")), concat!(
        "{\"type\":\"user\",\"uuid\":\"00000000-0000-4000-8000-000000000001\",\"parentUuid\":null}\n",
        "{\"type\":\"assistant\",\"uuid\":\"00000000-0000-4000-8000-000000000002\",\"parentUuid\":\"00000000-0000-4000-8000-000000000001\"}\n",
        "{\"type\":\"user\",\"uuid\":\"00000000-0000-4000-8000-000000000003\",\"parentUuid\":\"00000000-0000-4000-8000-000000000002\"}\n"
    )).unwrap();
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    assert!(
        !store::rewind::has_identity(&fixture.database.connection(), "session-a", turn.as_str())
            .unwrap()
    );
    let undo = service
        .undoable_turn(&owner, &turn)
        .unwrap()
        .expect("existing last-turn Undo");
    assert!(matches!(
        service.prepare_rewind(&owner, turn.clone(), undo, true, || {
            sdk::resolve_undo(&config, session, turn.as_str())
        }),
        Err(crate::acp::errors::SessionControlError::Refused(_))
    ));
    let (command, control) = service
        .prepare_rewind(&owner, turn.clone(), undo, false, || {
            sdk::resolve_undo(&config, session, turn.as_str())
        })
        .unwrap();
    assert_eq!(
        control,
        sage_acp_protocol::session_control::SessionControl::UndoTurn { turn_id: turn }
    );
    assert_eq!(
        command.cut_at.as_deref(),
        Some("00000000-0000-4000-8000-000000000002")
    );
    store::record_turn_undone(&mut fixture.database.connection(), &command).unwrap();
    assert!(fixture.page_turns().is_empty());
    assert_eq!(
        fixture.pending(),
        Some((command.turn_id.as_str().into(), store::UndoneThread::Cut))
    );
}

#[test]
fn runtime_checkpoint_undo_is_unavailable_after_restart() {
    let checkpoint = running("checkpoint-fixture");
    checkpoint.0.turn("turn-1", 0);
    assert_eq!(offer(&checkpoint, "turn-1"), (Some("turn-1".into()), true));
    assert!(composed_page(&checkpoint.0, &checkpoint.1).turns.as_slice()[0].conversation_target);
    checkpoint
        .0
        .database
        .connection()
        .execute(
            "UPDATE session_runtime_epochs SET state='exited',terminal_at=20 WHERE state='running'",
            [],
        )
        .unwrap();
    checkpoint.0.run();
    assert_eq!(offer(&checkpoint, "turn-1"), (None, false));
    assert!(!composed_page(&checkpoint.0, &checkpoint.1).turns.as_slice()[0].conversation_target);
}

#[test]
fn runtime_checkpoint_undo_cannot_undo_consecutively() {
    let checkpoint = running("checkpoint-fixture");
    checkpoint.0.turn("turn-1", 0);
    checkpoint.0.turn("turn-2", 1);
    assert_eq!(offer(&checkpoint, "turn-2"), (Some("turn-2".into()), true));
    checkpoint.0.undo("turn-2", true);
    assert_eq!(offer(&checkpoint, "turn-1"), (None, false));
    checkpoint.0.turn("turn-3", 2);
    assert_eq!(offer(&checkpoint, "turn-3"), (Some("turn-3".into()), true));
}

mod files {
    use super::super::{
        TurnUndo,
        route::{FILES_STAY, Rewind},
    };
    use crate::acp::{
        errors::SessionControlError,
        goal::NativeRpcReply,
        service::{AcpService, file_rewind::RESTORED_NOT_CUT},
        store::{AdapterStoreOwner, TurnUndoneCommand, agent_writes, rewind_tests::history},
        transcript_quote::Prompt,
    };
    use crate::project_registry::ProjectRegistryStore;
    use sage_acp_protocol::{
        contract::{RuntimeEpoch, SafeText, SageAgentSessionId, SageRootId, SageTurnId},
        intent::{ExternalId, SafeResourceRef},
        scheduler::RuntimeOwner,
        session_control::SessionControl,
    };
    use sha2::Digest;
    use std::{path::PathBuf, sync::Mutex};

    const AGENT: &str = "fn agent() {}\n";
    const BEFORE: &str = "fn before() {}\n";
    /// The first Claude Code release whose rewinds preview files.
    const PREVIEWS: &str = "2.1.326";

    /// One finished turn in which the agent wrote `a.rs`; the file now holds
    /// `on_disk`. A `legacy` turn predates rewind targets, so the seam cannot
    /// check it.
    struct Written {
        history: history::History,
        service: AcpService,
        owner: RuntimeOwner,
        turn: SageTurnId,
        file: PathBuf,
        undo: TurnUndo,
    }

    fn written(on_disk: &str, claude: &str, legacy: bool) -> Written {
        // The service starts first: its startup recovery would settle a
        // runtime it does not run, as in `running`.
        let fixture = crate::acp::store::turn_undo::fixture::Fixture::new();
        let service = AcpService::new(fixture.database.clone()).unwrap();
        fixture.run();
        let rewind = crate::acp::bindings::claude::sdk::rewind_capability(claude);
        let mut capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1 =
            serde_json::from_str(crate::acp::store::turn_undo::fixture::CAPABILITIES).unwrap();
        capabilities.rewind = rewind;
        fixture
            .database
            .connection()
            .execute(
                "UPDATE agent_sessions SET capabilities_json=?1 WHERE id='session-a'",
                [serde_json::to_string(&capabilities).unwrap()],
            )
            .unwrap();
        let mut history = history::History {
            fixture,
            turns: Vec::new(),
        };
        let prompt = Prompt::new("prompt", "edit a.rs").unwrap();
        if legacy {
            history.historical_turn(prompt, None);
        } else {
            history.turn(prompt, None);
        }
        let fixture = &history.fixture;
        let turn = history.turns[0].clone();
        let owner = RuntimeOwner {
            project_id: fixture.project.clone(),
            chat_id: fixture.chat.clone(),
            session_id: SageAgentSessionId::parse("session-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
        };
        let root = fixture
            .database
            .root_records(fixture.project.as_str())
            .unwrap()
            .remove(0);
        let file = root.absolute_path.join("a.rs");
        std::fs::write(&file, on_disk).unwrap();
        let mut connection = fixture.database.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        agent_writes::record(
            &transaction,
            &AdapterStoreOwner {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                turn_id: turn.clone(),
                binding_digest: "c2".repeat(32),
                external_session_id: ExternalId::new("externalSessionId", "thread-a").unwrap(),
            },
            &SafeResourceRef::new(SageRootId::parse(&root.id).unwrap(), "a.rs").unwrap(),
            Some(&sha2::Sha256::digest(AGENT.as_bytes()).into()),
            1,
        )
        .unwrap();
        transaction.commit().unwrap();
        drop(connection);
        Written {
            history,
            service,
            owner,
            turn,
            file,
            undo: TurnUndo::from_capability(rewind).unwrap(),
        }
    }

    type Outcome = Result<(TurnUndoneCommand, Option<SafeText<512>>, bool), SessionControlError>;

    impl Written {
        /// Route `mode` for the turn as `apply_rewind` does, against a harness
        /// whose dry run lists `a.rs`, whose restore writes it back, whose
        /// own undo answers, and whose cut answers `cut`. Returns the outcome
        /// and the steps the harness saw.
        fn rewind(
            &self,
            mode: Rewind,
            cut: fn() -> Result<NativeRpcReply, SessionControlError>,
        ) -> (Outcome, Vec<&'static str>) {
            let steps = Mutex::new(Vec::new());
            let (file, turn) = (&self.file, &self.turn);
            let mut control = |control: SessionControl| {
                let (step, reply) = match control {
                    SessionControl::RewindFiles { dry_run: true, .. } => (
                        "dry run",
                        Ok(NativeRpcReply::FileRewindPlan(vec![file.clone()])),
                    ),
                    SessionControl::RewindFiles { .. } => {
                        std::fs::write(file, BEFORE).unwrap();
                        (
                            "restore",
                            Ok(NativeRpcReply::FileRewindPlan(vec![file.clone()])),
                        )
                    }
                    SessionControl::RewindConversation { before } => {
                        assert_eq!(&before.turn_id, turn);
                        ("cut", cut())
                    }
                    SessionControl::UndoTurn { turn_id } => {
                        assert_eq!(&turn_id, turn);
                        ("undo", Ok(NativeRpcReply::Controlled(None)))
                    }
                    _ => panic!("only undo, file and conversation rewinds"),
                };
                steps.lock().unwrap().push(step);
                reply
            };
            let outcome = tauri::async_runtime::block_on(self.service.rewind_turn(
                &self.owner,
                &self.turn,
                self.undo,
                mode,
                &mut control,
                || Ok::<_, String>(None),
            ));
            (outcome, steps.into_inner().unwrap())
        }

        /// Nothing is pending and the turn is still on the page.
        fn untouched(&self) {
            let fixture = &self.history.fixture;
            crate::acp::store::turn_undo::intent::ensure_settled(
                &fixture.database.connection(),
                fixture.project.as_str(),
                fixture.chat.as_str(),
            )
            .unwrap();
            assert_eq!(fixture.page_turns(), [self.turn.as_str()]);
        }

        /// Recording the staged undo takes the turn off the page.
        fn cut(&self, command: &TurnUndoneCommand) {
            let fixture = &self.history.fixture;
            crate::acp::store::record_turn_undone(&mut fixture.database.connection(), command)
                .unwrap();
            assert!(fixture.page_turns().is_empty());
        }

        fn on_disk(&self) -> String {
            std::fs::read_to_string(&self.file).unwrap()
        }
    }

    const UNDO: Rewind = Rewind::Undo { keep_files: false };
    const CUT: fn() -> Result<NativeRpcReply, SessionControlError> =
        || Ok(NativeRpcReply::Controlled(None));

    #[test]
    fn undo_refuses_a_file_changed_since_the_agent_wrote_it_and_leaves_it_untouched() {
        let written = written("fn user() {}\n", PREVIEWS, false);
        let (outcome, steps) = written.rewind(UNDO, CUT);
        assert!(matches!(
            outcome,
            Err(SessionControlError::Refused(Some(text)))
                if text.as_str().starts_with("1 of the files changed since the agent wrote them")
        ));
        assert_eq!(steps, ["dry run"]);
        assert_eq!(written.on_disk(), "fn user() {}\n");
        written.untouched();
        // Keeping the files instead cuts the conversation with no native call.
        let (outcome, steps) = written.rewind(Rewind::Undo { keep_files: true }, CUT);
        let (command, summary, restored) = outcome.unwrap();
        assert!(steps.is_empty() && !restored);
        assert_eq!(summary.unwrap().as_str(), FILES_STAY);
        assert_eq!(written.on_disk(), "fn user() {}\n");
        written.cut(&command);
    }

    #[test]
    fn undo_with_clean_files_restores_them_then_cuts_the_turn() {
        let written = written(AGENT, PREVIEWS, false);
        let (outcome, steps) = written.rewind(UNDO, CUT);
        let (command, summary, restored) = outcome.unwrap();
        assert_eq!(steps, ["dry run", "restore", "cut"]);
        assert!(restored);
        assert_eq!(summary.unwrap().as_str(), "Restored 1 file");
        written.cut(&command);
    }

    #[test]
    fn undo_of_a_turn_the_seam_cannot_check_cuts_the_conversation_and_keeps_files() {
        let written = written(AGENT, PREVIEWS, true);
        let (outcome, steps) = written.rewind(UNDO, CUT);
        let (command, summary, restored) = outcome.unwrap();
        assert!(steps.is_empty() && !restored);
        assert_eq!(summary.unwrap().as_str(), FILES_STAY);
        assert_eq!(written.on_disk(), AGENT);
        written.cut(&command);
    }

    #[test]
    fn undo_without_a_file_preview_keeps_the_bindings_own_undo() {
        let written = written(AGENT, "2.1.300", false);
        let (outcome, steps) = written.rewind(UNDO, CUT);
        let (command, _, restored) = outcome.unwrap();
        assert_eq!(steps, ["undo"]);
        assert!(!restored);
        written.cut(&command);
    }

    #[test]
    fn a_cut_refused_after_files_are_restored_rolls_back_and_says_the_files_were_restored() {
        let written = written(AGENT, PREVIEWS, false);
        let (outcome, steps) = written.rewind(Rewind::Conversation { files: true }, || {
            Err(SessionControlError::Refused(None))
        });
        assert!(matches!(
            outcome,
            Err(SessionControlError::Refused(Some(text))) if text.as_str() == RESTORED_NOT_CUT
        ));
        assert_eq!(steps, ["dry run", "restore", "cut"]);
        written.untouched();
    }
}

