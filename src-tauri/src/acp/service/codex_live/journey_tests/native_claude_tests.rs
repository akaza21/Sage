use super::*;
use crate::acp::bindings::registry::{BindingProjectRoot, BindingRegistry};
use crate::acp::runtime::{HostCancelIntent, HostOutcome, HostPromptIntent};
use sage_acp_protocol::lifecycle::Residency;
use std::fs;

#[test]
fn native_claude_subprocess_binding_pump_sqlite_stream_cancel_and_permissions() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        for case in [
            "claude-turn-completed",
            "claude-thinking-live",
            "claude-approval-approved",
            "claude-approval-declined",
        ] {
            let directory =
                std::env::temp_dir().join(format!("sage-nc1-journey-{}", uuid::Uuid::new_v4()));
            let project = directory.join("project");
            let profile = directory.join("profile");
            fs::create_dir_all(project.join("src")).unwrap();
            fs::create_dir_all(profile.join("durable")).unwrap();
            fs::write(
                profile.join(".sage-disposable-profile-v1"),
                b"sage-disposable-profile-v1\n",
            )
            .unwrap();
            let artifact = std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("acp-adversarial-fixture.exe");
            let hash = crate::harness_discovery::sha256_file(&artifact)
                .expect("coordinator must build the acp-fixture executable");
            let mut sha256 = [0; 32];
            for (index, byte) in sha256.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap();
            }
            let selection = crate::launch_profile::FixtureSelection {
                proof_root: directory.canonicalize().unwrap(),
                profile_root: profile.canonicalize().unwrap(),
                durable_dir: profile.join("durable").canonicalize().unwrap(),
                project_root: project.canonicalize().unwrap(),
                artifact_bytes: fs::metadata(&artifact).unwrap().len(),
                artifact_path: artifact.canonicalize().unwrap(),
                artifact_sha256: sha256,
                case: case.into(),
            };
            let registry =
                Arc::new(BindingRegistry::desktop_claude_fixture(selection.clone()).unwrap());
            assert!(registry.resolve("codex-app-server").is_err());
            let database_path = profile.join("durable/sage.db");
            let setup = FixtureSetup::open_with_registry_at_root(
                &database_path,
                Some(database_path.clone()),
                registry,
                &project,
            )
            .await;
            let root = || {
                BindingProjectRoot::Claude(
                    ProjectRootAuthority::for_test(
                        setup.project_id.clone(),
                        ProjectGeneration::ZERO,
                        setup.root_id.clone(),
                        &setup.root_path,
                    )
                    .unwrap(),
                )
            };
            let revision = setup
                .service
                .load_chat_page(setup.project_id.clone(), setup.chat_id.clone(), None)
                .unwrap()
                .projection_revision;
            assert_eq!(
                revision,
                ProjectionRevision::ZERO,
                "the empty project chat starts at projection revision zero"
            );
            let mut start = setup.start_command_for(&setup.chat_id, revision);
            start.binding_id = BindingId::new("bindingId", "claude-agent-acp").unwrap();
            let started = setup
                .service
                .start_session(
                    setup
                        .service
                        .binding_registry()
                        .resolve("claude-agent-acp")
                        .unwrap(),
                    &root(),
                    start,
                )
                .await
                .unwrap();
            let owner = RuntimeOwner {
                project_id: setup.project_id.clone(),
                chat_id: setup.chat_id.clone(),
                session_id: started.session_id,
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            };
            setup
                .service
                .activate_session(
                    setup
                        .service
                        .binding_registry()
                        .resolve("claude-agent-acp")
                        .unwrap(),
                    setup.activate_command(&owner),
                    root(),
                )
                .await
                .unwrap();
            setup.wait_for_ready(&owner);
            let completed_before = if case.starts_with("claude-approval") {
                let previous = setup.completed_turns(&owner);
                setup
                    .service
                    .send_session_turn(HostPromptIntent {
                        command: setup.turn_command(&owner),
                        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
                    })
                    .unwrap();
                setup.wait_for_completed_after(&owner, &previous);
                setup.completed_turns(&owner)
            } else {
                Vec::new()
            };
            setup
                .service
                .send_session_turn(HostPromptIntent {
                    command: setup.turn_command(&owner),
                    access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
                })
                .unwrap();
            if case == "claude-thinking-live" {
                let turn = setup.wait_for_running_turn(&owner);
                setup.wait_until(|| {
                    session_items(&setup, &owner)
                        .iter()
                        .any(|item| item.kind == "thoughtSummary")
                });
                setup
                    .service
                    .cancel_session_turn(HostCancelIntent {
                        owner: owner.clone(),
                        turn_id: turn,
                        client_intent_id: ClientIntentId::new(),
                        now_millis: sage_core::time::now_millis(),
                    })
                    .unwrap();
                setup.wait_for_cancelled(&owner);
            } else if case.starts_with("claude-approval") {
                setup.wait_until(|| setup.service.database.connection().query_row("SELECT COUNT(*) FROM server_requests WHERE kind='permission' AND state IN ('received','asked')",[],|row|row.get::<_,i64>(0)).unwrap() == 1);
                let (request, turn): (String, String) = setup
                    .service
                    .database
                    .connection()
                    .query_row(
                        "SELECT id,turn_id FROM server_requests WHERE kind='permission'",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                setup
                    .service
                    .resolve_permission_decision(crate::acp::service::PermissionDecisionIntent {
                        project_id: owner.project_id.clone(),
                        chat_id: owner.chat_id.clone(),
                        session_id: owner.session_id.clone(),
                        runtime_epoch: owner.runtime_epoch,
                        turn_id: SageTurnId::parse(turn).unwrap(),
                        request_id: sage_acp_protocol::contract::SageServerRequestId::parse(
                            request,
                        )
                        .unwrap(),
                        decision: if case.ends_with("declined") {
                            sage_acp_protocol::permissions::PermissionDecision::Deny
                        } else {
                            sage_acp_protocol::permissions::PermissionDecision::AllowOnce
                        },
                    })
                    .unwrap();
                setup.wait_for_completed_after(&owner, &completed_before);
            } else {
                setup.wait_for_completed_after(&owner, &[]);
                assert!(
                    session_items(&setup, &owner)
                        .iter()
                        .any(|item| item.kind == "text" && item.lifecycle == "completed")
                );
            }
            let before = setup.page(&owner);
            let expected_state = if case == "claude-thinking-live" {
                let stopped = before
                    .sessions
                    .as_slice()
                    .iter()
                    .find(|session| session.id == owner.session_id)
                    .unwrap();
                assert_eq!(stopped.state, SessionState::Ready);
                assert_eq!(stopped.residency, Residency::Cold);
                assert_eq!(stopped.runtime_epoch, None);
                assert!(!setup.service.lane_at(0).runtime.is_active());
                SessionState::Ready
            } else {
                let closed = setup
                    .service
                    .close_session(setup.close_command(&owner))
                    .unwrap_or_else(|error| {
                        panic!("Claude fixture {case} close failed: {error:?}")
                    });
                assert_eq!(closed, HostOutcome::Closed);
                SessionState::Closed
            };
            setup.wait_for_job_zero();
            setup.service.shutdown_blocking().unwrap();
            drop(setup);
            assert!(selection.consume_one_shot().is_err());
            let registry = Arc::new(BindingRegistry::desktop_claude_fixture(selection).unwrap());
            let reopened = AcpService::new_with_registry_for_test(
                crate::persistence::Database::open(&database_path).unwrap(),
                registry,
            )
            .unwrap();
            let restored = reopened
                .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
                .unwrap();
            let session = restored
                .sessions
                .as_slice()
                .iter()
                .find(|session| session.id == owner.session_id)
                .unwrap();
            assert_eq!(session.binding_id.as_str(), "claude-agent-acp");
            assert_eq!(session.state, expected_state);
            assert_eq!(session.residency, Residency::Cold);
            assert_eq!(session.runtime_epoch, None);
            assert_eq!(restored.turns, before.turns);
            let persisted: String = reopened
                .database
                .connection()
                .query_row(
                    "SELECT external_session_id FROM agent_sessions WHERE id=?1",
                    [owner.session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(persisted, "fixture-persisted-session-1");
            reopened.shutdown_blocking().unwrap();
            drop(reopened);
            fs::remove_dir_all(directory).unwrap();
        }
    });
}
