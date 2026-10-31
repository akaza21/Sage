use super::bootstrap_pause::PauseObservation;
use super::bootstrap_support::*;
use super::*;
use crate::acp::{
    browser::{
        BrowserAction, BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest,
        BrowserResult, BrowserScope, FillSecret, permission::BrowserPermissionChoice,
        session_capability::attached::AttachedSessionCapability,
    },
    contract::SageAgentSessionId,
    runtime::BoundedThread,
    service::browser_permission::BrowserPermissionOwner,
    store::DatabaseAcpStore,
};
use crate::browser::state_query::bootstrap_fixture::BrowserDocumentFixture;
use std::sync::{Arc, Mutex, mpsc};

fn permission(events: &mpsc::Receiver<Event>) -> crate::acp::contract::SageServerRequestId {
    for _ in 0..64 {
        match next(events) {
            Event::Permission(id) => return id,
            Event::BrowserDone(result) => {
                panic!("browser ended before published approval: {result:?}")
            }
            _ => {}
        }
    }
    panic!("bounded event budget exceeded")
}

fn outcome(events: &mpsc::Receiver<Event>) -> Result<BrowserResult, BrowserFailure> {
    for _ in 0..64 {
        match next(events) {
            Event::BrowserDone(result) => return result,
            Event::Permission(_) => panic!("unexpected repeated browser approval"),
            _ => {}
        }
    }
    panic!("bounded event budget exceeded")
}

fn allow(
    setup: &FixtureSetup,
    owner: &BrowserPermissionOwner,
    id: crate::acp::contract::SageServerRequestId,
) {
    let page = setup
        .service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    let turn = page
        .turns
        .as_slice()
        .iter()
        .find(|turn| turn.id == owner.turn_id)
        .unwrap();
    let item = turn
        .items
        .as_slice()
        .iter()
        .find(|item| {
            matches!(&item.content,
        ItemContent::PermissionRequest { request_id,
            state: crate::acp::lifecycle::ServerRequestState::Received,
            source: crate::acp::projection::PermissionRequestSource::Browser, .. }
            if request_id == &id)
        })
        .expect("visible received browser approval");
    assert_eq!(
        (
            &item.project_id,
            &item.chat_id,
            &item.session_id,
            item.runtime_epoch
        ),
        (
            &owner.project_id,
            &owner.chat_id,
            &owner.session_id,
            owner.runtime_epoch
        )
    );
    let response = setup
        .service
        .resolve_browser_permission_decision(owner.clone(), id, BrowserPermissionChoice::AllowOnce)
        .unwrap();
    assert_eq!(
        serde_json::to_value(response).unwrap()["state"],
        "resolvedAllowed"
    );
}

#[test]
fn first_chat_real_turn_two_bootstrap_authorizes_snapshot_fill_without_replay() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let mut setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-browser-bootstrap",
            ]))
            .await;
        assert_eq!(
            setup
                .service
                .database
                .bootstrap_snapshot()
                .unwrap()
                .tabs
                .len(),
            1
        );
        let (sender, events) = mpsc::sync_channel(64);
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let database = setup.service.database.clone();
        let store = CausalStore {
            inner: DatabaseAcpStore::new(database.clone()),
            database,
            project: setup.project_id.clone(),
            chat: setup.chat_id.clone(),
            events: sender.clone(),
            release: release_receiver,
            prepared: 0,
            states: Vec::new(),
        };
        install_store(&mut setup.service, store);
        let setup = Arc::new(setup);
        let _cleanup = Cleanup(setup.clone());
        let mut release = Release(Some(release_sender));
        let runtime_owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let context = Context {
            setup: setup.clone(),
            document: Arc::new(BrowserDocumentFixture::new()),
            session: runtime_owner.session_id.clone(),
        };
        let (url, revision) = context.document.settled_tab("browser-1").unwrap();
        let attach = || {
            setup
                .service
                .attach_browser_tab(
                    setup.project_id.clone(),
                    setup.chat_id.clone(),
                    "browser-1",
                    &url,
                    revision,
                )
                .unwrap()
        };
        attach();
        let scope = BrowserScope {
            session_id: runtime_owner.session_id.as_str().into(),
            tab_id: 1,
        };
        let snapshot = BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        };
        let trace = Arc::new(Mutex::new(Trace::default()));
        trace.lock().unwrap().browser.open_tab(scope.clone());

        // First real prompt, protocol completion, and production terminal handling.
        setup
            .service
            .send_codex_turn(setup.turn_command(&runtime_owner))
            .unwrap();
        let first = loop {
            match next(&events) {
                Event::Turn(id, TurnState::Completed) => break id,
                Event::Turn(_, _) => {}
                event => panic!("unexpected first-turn event: {event:?}"),
            }
        };
        assert_eq!(setup.completed_turns(&runtime_owner), vec![first.clone()]);
        let worker_setup = setup.clone();
        let worker_owner = runtime_owner.clone();
        let mut send = BoundedThread::spawn("bootstrap-second-prompt", move || {
            worker_setup
                .service
                .send_codex_turn(worker_setup.turn_command(&worker_owner))
                .unwrap();
        })
        .unwrap();
        let Event::Prepared(second) = next(&events) else {
            panic!("turn-two commit barrier")
        };
        assert_ne!(first, second);
        assert!(
            setup
                .page(&runtime_owner)
                .turns
                .as_slice()
                .iter()
                .any(|t| t.id == second && t.state == TurnState::Queued)
        );
        assert!(
            setup
                .service
                .lane_at(0)
                .scheduler
                .active()
                .unwrap()
                .turn_id
                .is_none()
        );
        let prebind = perform(context.clone(), trace.clone(), &snapshot);
        assert_eq!(prebind, Err(BrowserFailure::Denied));
        assert_eq!(trace.lock().unwrap().dispatched, 0);
        assert_eq!(trace.lock().unwrap().issued, 0);
        release.once();
        let running = next(&events);
        assert!(
            matches!(&running, Event::Turn(id, TurnState::Running) if id == &second),
            "unexpected turn-two bootstrap event: {running:?}"
        );
        send.wait_and_join(WAIT).unwrap();
        assert_eq!(
            trace.lock().unwrap().issued,
            0,
            "prebind request never replays after binding"
        );
        let owner = setup
            .service
            .current_browser_owner(&setup.project_id, &setup.chat_id, &runtime_owner.session_id)
            .unwrap();
        assert_eq!(owner.turn_id, second);
        assert!(setup.service.browser_owner_witness_matches(&owner));
        context.document.late_metadata();

        let mut foreign_scope = snapshot.clone();
        foreign_scope.scope.session_id = "wrong-session".into();
        assert_eq!(
            perform(context.clone(), trace.clone(), &foreign_scope),
            Err(BrowserFailure::Denied)
        );
        for (project, chat) in [
            (
                SageProjectId::parse("wrong-project").unwrap(),
                setup.chat_id.clone(),
            ),
            (
                setup.project_id.clone(),
                SageChatId::parse("wrong-chat").unwrap(),
            ),
        ] {
            let mut factory = Factory(trace.clone());
            assert!(
                AttachedSessionCapability::new(
                    context.clone(),
                    project,
                    chat,
                    runtime_owner.session_id.clone(),
                    &mut factory
                )
                .perform(&snapshot)
                .is_err()
            );
        }
        assert_eq!(trace.lock().unwrap().issued, 0);
        let call = Call::spawn(
            context.clone(),
            trace.clone(),
            snapshot.clone(),
            sender.clone(),
        );
        let request_id = permission(&events);
        let mut foreign = vec![owner.clone(); 5];
        foreign[0].project_id = SageProjectId::parse("wrong-project").unwrap();
        foreign[1].chat_id = SageChatId::parse("wrong-chat").unwrap();
        foreign[2].session_id = SageAgentSessionId::parse("wrong-session").unwrap();
        foreign[3].runtime_epoch = RuntimeEpoch::new(2).unwrap();
        foreign[4].turn_id = first;
        for wrong in foreign {
            assert!(
                setup
                    .service
                    .resolve_browser_permission_decision(
                        wrong,
                        request_id.clone(),
                        BrowserPermissionChoice::AllowOnce
                    )
                    .is_err()
            );
            assert_eq!(trace.lock().unwrap().issued, 0);
        }
        allow(&setup, &owner, request_id);
        let BrowserResult::SnapshotPage { nodes, .. } = outcome(&events).unwrap() else {
            panic!("snapshot result")
        };
        call.finish();
        let fill = BrowserRequest {
            scope,
            command: BrowserCommand::Act {
                action: BrowserAction::Fill {
                    handle: nodes[0].handle.clone(),
                    value: FillSecret::new("turn two text".into()),
                },
            },
        };
        let call = Call::spawn(context.clone(), trace.clone(), fill.clone(), sender.clone());
        allow(&setup, &owner, permission(&events));
        assert_eq!(outcome(&events), Ok(BrowserResult::Act { applied: true }));
        call.finish();
        assert_eq!(trace.lock().unwrap().issued, 2);

        // Hand back is chat/tab-scoped and restores a fresh authorized request.
        assert!(setup.service.take_over_browser_tab("browser-1"));
        let other_chat = setup.create_chat();
        setup
            .service
            .attach_browser_tab(
                setup.project_id.clone(),
                other_chat,
                "browser-2",
                &url,
                revision,
            )
            .unwrap();
        assert!(setup.service.take_over_browser_tab("browser-2"));
        let pause = PauseObservation::arm(&setup.service.database);
        let call = Call::spawn(
            context.clone(),
            trace.clone(),
            snapshot.clone(),
            sender.clone(),
        );
        pause.next(4);
        assert!(setup.service.hand_back_browser_tab("browser-2"));
        assert!(
            setup
                .service
                .browser_chat_under_user_control(&setup.project_id, &setup.chat_id)
        );
        pause.next(5);
        assert_eq!(trace.lock().unwrap().dispatched, 2);
        assert!(
            events.try_recv().is_err(),
            "another Chat's Hand back cannot resume this call"
        );
        assert!(setup.service.hand_back_browser_tab("browser-1"));
        drop(pause);
        allow(&setup, &owner, permission(&events));
        assert!(matches!(
            outcome(&events),
            Ok(BrowserResult::SnapshotPage { .. })
        ));
        call.finish();
        assert_eq!(trace.lock().unwrap().issued, 3);

        for detach in [false, true] {
            attach();
            let call = Call::spawn(
                context.clone(),
                trace.clone(),
                snapshot.clone(),
                sender.clone(),
            );
            let pending = permission(&events);
            if detach {
                setup
                    .service
                    .detach_browser_tab(&setup.project_id, &setup.chat_id)
                    .unwrap();
            } else {
                setup.service.revoke_browser_tab("browser-1");
            } // Stop's service operation.
            assert_eq!(outcome(&events), Err(BrowserFailure::Denied));
            call.finish();
            assert!(
                setup
                    .service
                    .resolve_browser_permission_decision(
                        owner.clone(),
                        pending,
                        BrowserPermissionChoice::AllowOnce
                    )
                    .is_err()
            );
            assert_eq!(
                trace.lock().unwrap().issued,
                3,
                "revoked request cannot issue"
            );
        }
        attach();
        trace.lock().unwrap().unknown = true;
        let call = Call::spawn(context.clone(), trace.clone(), fill, sender.clone());
        allow(&setup, &owner, permission(&events));
        assert_eq!(outcome(&events), Err(BrowserFailure::OutcomeUnknown));
        call.finish();
        assert_eq!(
            trace.lock().unwrap().issued,
            4,
            "issued unknown call never replays on Hand back"
        );
        assert_eq!(trace.lock().unwrap().dispatched, 4);
        assert_eq!(trace.lock().unwrap().browser.action_count(), 2);
        assert!(events.try_recv().is_err(), "no extra approval or replay");

        setup
            .service
            .cancel_codex_turn(HostCancelIntent {
                owner: runtime_owner.clone(),
                turn_id: second,
                client_intent_id: ClientIntentId::new(),
                now_millis: crate::persistence::now_millis(),
            })
            .unwrap();
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}
