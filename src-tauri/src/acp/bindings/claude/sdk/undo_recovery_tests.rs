use super::*;

#[test]
fn a_prompt_that_may_have_been_delivered_after_an_undo_relaunch_retires_the_session() {
    for observation in [
        None,
        Some(store::SendObservation::CompleteBytes),
        Some(store::SendObservation::PossibleBytes),
    ] {
        let chat = Chat::new(true);
        let command = chat.stage(LAST).unwrap();
        store::record_turn_undone(&mut chat.fixture.database.connection(), &command).unwrap();
        crate::acp::recovery::recover_after_restart(&mut chat.fixture.database.connection(), 70)
            .unwrap();
        let session = SageAgentSessionId::parse("session-a").unwrap();
        chat.activate(&session, 3, SESSION);
        let prepared = chat.send(session.clone(), 3).unwrap();
        let mut sending = store::TurnMutationCommand {
            project_id: chat.fixture.project.clone(),
            chat_id: chat.fixture.chat.clone(),
            session_id: session.clone(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 110,
        };
        let started =
            store::begin_send(&mut chat.fixture.database.connection(), sending.clone()).unwrap();
        if let Some(observation) = observation {
            sending.expected_revision = started.revision;
            store::record_send_observation(
                &mut chat.fixture.database.connection(),
                sending,
                observation,
            )
            .unwrap();
        }
        assert!(chat.fixture.pending().is_some());
        let reopened =
            crate::persistence::Database::open(&chat.fixture.root.join("sage.db")).unwrap();
        crate::acp::recovery::recover_after_restart(&mut reopened.connection(), 120).unwrap();
        let page = store::load_chat_page(
            &reopened.connection(),
            chat.fixture.project.clone(),
            chat.fixture.chat.clone(),
            None,
        )
        .unwrap();
        assert!(page.current_session_id.is_none(), "{observation:?}");
        let turn = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.id == prepared.turn_id)
            .unwrap();
        assert_eq!(turn.state, TurnState::DeliveryUnknown, "{observation:?}");
        assert_eq!(
            chat.fixture.page_turns(),
            [prepared.turn_id.as_str(), FIRST]
        );
        assert!(matches!(
            chat.send(session, 3),
            Err(StoreError::Conflict {
                resource: "sessionEpoch"
            })
        ));
        assert_eq!(
            crate::acp::recovery::recover_after_restart(&mut reopened.connection(), 130)
                .unwrap()
                .recovered_sessions,
            0
        );
    }
}

#[test]
fn a_crash_before_undo_dispatch_keeps_the_turns_files_and_transcript() {
    for parked in [false, true] {
        let chat = Chat::new(true);
        let edited = chat.fixture.root.join("edited.txt");
        let created = chat.fixture.root.join("created.txt");
        std::fs::write(&edited, "before the turn\n").unwrap();
        std::fs::write(&edited, "edited by the last turn\n").unwrap();
        std::fs::write(&created, "created by the last turn\n").unwrap();
        let transcript = std::fs::read(&chat.transcript).unwrap();
        let command = chat.stage(LAST).unwrap();
        if parked {
            store::park_session_after_zero_process(
                &mut chat.fixture.database.connection(),
                store::ParkSessionCommand {
                    project_id: command.project_id,
                    chat_id: command.chat_id,
                    session_id: command.session_id,
                    runtime_epoch: command.runtime_epoch,
                    exit_kind: RequiredText::new("exitKind", "shutdown").unwrap(),
                    now_millis: 60,
                },
            )
            .unwrap();
        }
        let reopened =
            crate::persistence::Database::open(&chat.fixture.root.join("sage.db")).unwrap();
        crate::acp::recovery::recover_after_restart(&mut reopened.connection(), 70).unwrap();
        assert_eq!(chat.fixture.page_turns(), [LAST, FIRST]);
        assert_eq!(
            std::fs::read_to_string(edited).unwrap(),
            "edited by the last turn\n"
        );
        assert_eq!(
            std::fs::read_to_string(created).unwrap(),
            "created by the last turn\n"
        );
        assert_eq!(std::fs::read(&chat.transcript).unwrap(), transcript);
        assert!(chat.fixture.pending().is_none());
        store::turn_undo::intent::ensure_settled(
            &reopened.connection(),
            chat.fixture.project.as_str(),
            chat.fixture.chat.as_str(),
        )
        .unwrap();
    }
}

#[test]
fn undo_interrupted_after_persisting_the_intent_recovers_on_restart() {
    for (parked, confirmed, earlier_cut) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, false, true),
        (true, false, true),
    ] {
        let chat = Chat::new(true);
        if earlier_cut {
            let command = chat.stage(LAST).unwrap();
            store::record_turn_undone(&mut chat.fixture.database.connection(), &command).unwrap();
        }
        let command = chat.stage(if earlier_cut { FIRST } else { LAST }).unwrap();
        if confirmed {
            store::record_turn_undone(&mut chat.fixture.database.connection(), &command).unwrap();
        }
        let visible = if confirmed || earlier_cut {
            vec![FIRST]
        } else {
            vec![LAST, FIRST]
        };
        assert_eq!(chat.fixture.page_turns(), visible);
        let session = SageAgentSessionId::parse("session-a").unwrap();
        assert!(matches!(
            chat.send(session.clone(), 2),
            Err(StoreError::Conflict { resource })
                if resource == if confirmed { "sessionEpoch" } else { "unsettledUndo" }
        ));
        if parked {
            store::park_session_after_zero_process(
                &mut chat.fixture.database.connection(),
                store::ParkSessionCommand {
                    project_id: chat.fixture.project.clone(),
                    chat_id: chat.fixture.chat.clone(),
                    session_id: session.clone(),
                    runtime_epoch: RuntimeEpoch::new(2).unwrap(),
                    exit_kind: RequiredText::new("exitKind", "shutdown").unwrap(),
                    now_millis: 60,
                },
            )
            .unwrap();
        }
        let reopened =
            crate::persistence::Database::open(&chat.fixture.root.join("sage.db")).unwrap();
        crate::acp::recovery::recover_after_restart(&mut reopened.connection(), 70).unwrap();
        if !confirmed {
            assert_eq!(chat.fixture.page_turns(), visible);
            let pending = earlier_cut.then(|| (LAST.to_owned(), store::UndoneThread::Cut));
            assert_eq!(chat.fixture.pending(), pending);
            let page = store::load_chat_page(
                &reopened.connection(),
                chat.fixture.project.clone(),
                chat.fixture.chat.clone(),
                None,
            )
            .unwrap();
            assert!(
                page.current_session_id.is_none(),
                "uncertain session is retired"
            );
            store::turn_undo::intent::ensure_settled(
                &reopened.connection(),
                chat.fixture.project.as_str(),
                chat.fixture.chat.as_str(),
            )
            .unwrap();
            assert!(matches!(
                chat.send(session, 2),
                Err(StoreError::Conflict {
                    resource: "sessionEpoch"
                })
            ));
            continue;
        }
        assert_eq!(chat.fixture.page_turns(), [FIRST]);
        let rewind = store::pending_thread_rewind(&reopened.connection(), "session-a")
            .unwrap()
            .unwrap();
        assert_eq!(rewind.cut_at.as_deref(), Some(REPLY));
        let launch = SessionLaunch {
            session_id: SESSION.into(),
            resume: true,
            model: None,
            rewind_at: rewind.cut_at,
        };
        assert_eq!(
            launch.session_args(Some(&chat.config)).unwrap(),
            ["--resume", SESSION, "--resume-session-at", REPLY].map(std::ffi::OsString::from)
        );
        let saved = std::fs::read(&chat.transcript).unwrap();
        std::fs::remove_file(&chat.transcript).unwrap();
        assert!(launch.session_args(Some(&chat.config)).is_err());
        assert!(
            store::pending_thread_rewind(&reopened.connection(), "session-a")
                .unwrap()
                .is_some()
        );
        std::fs::write(&chat.transcript, saved).unwrap();
        assert!(launch.session_args(Some(&chat.config)).is_ok());
        chat.activate(&session, 3, SESSION);
        chat.send(session, 3).unwrap();
        assert!(
            store::pending_thread_rewind(&reopened.connection(), "session-a")
                .unwrap()
                .is_some()
        );
    }
    for parked in [false, true] {
        let fixture = Fixture::on(crate::acp::bindings::codex::CODEX_BINDING_ID);
        fixture.run();
        fixture.turn(FIRST, 0);
        let owner = crate::acp::scheduler::RuntimeOwner {
            project_id: fixture.project.clone(),
            chat_id: fixture.chat.clone(),
            session_id: SageAgentSessionId::parse("session-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        };
        crate::acp::turn_undo::stage_turn_undo(
            &fixture.database,
            &owner,
            SageTurnId::parse(FIRST).unwrap(),
            true,
            || Ok::<_, crate::acp::bindings::claude::sdk::TranscriptError>(None),
        )
        .unwrap();
        if parked {
            store::park_session_after_zero_process(
                &mut fixture.database.connection(),
                store::ParkSessionCommand {
                    project_id: fixture.project.clone(),
                    chat_id: fixture.chat.clone(),
                    session_id: owner.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    exit_kind: RequiredText::new("exitKind", "shutdown").unwrap(),
                    now_millis: 60,
                },
            )
            .unwrap();
        }
        let reopened = crate::persistence::Database::open(&fixture.root.join("sage.db")).unwrap();
        crate::acp::recovery::recover_after_restart(&mut reopened.connection(), 70).unwrap();
        assert_eq!(fixture.page_turns(), [FIRST]);
        store::turn_undo::intent::ensure_settled(
            &reopened.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str(),
        )
        .unwrap();
        let revision = fixture.revision();
        let send = store::prepare_turn(
            &mut reopened.connection(),
            store::PrepareTurnCommand {
                project_id: fixture.project.clone(),
                project_generation: ProjectGeneration::new(1).unwrap(),
                chat_id: fixture.chat.clone(),
                session_id: owner.session_id,
                runtime_epoch: owner.runtime_epoch,
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::new(),
                prompt: Prompt::new("prompt", "This thread must not accept another send").unwrap(),
                now_millis: 80,
            },
        );
        assert!(matches!(
            send,
            Err(StoreError::Conflict {
                resource: "sessionEpoch"
            })
        ));
    }
}
