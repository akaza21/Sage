use super::*;
use crate::acp::{
    bindings::registry::{BindingProjectRoot, BindingRegistry},
    projection::TextRole,
    runtime::{HostOutcome, HostPromptIntent},
};
use sage_acp_protocol::session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigValue};
use std::fs;

#[test]
fn native_pi_subprocess_real_binding_pump_sqlite_settlement_and_idle_configuration() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        for case in [
            "pi-idle-config-journey",
            "pi-compaction-started",
            "pi-compaction-completed",
            "pi-compaction-failed",
            "pi-compaction-cancelled",
            "pi-non-read-denial",
            "pi-cancelled-dialog",
            "pi-turn-retry",
        ] {
            let directory =
                std::env::temp_dir().join(format!("sage-nc1b-journey-{}", uuid::Uuid::new_v4()));
            let project = directory.join("project");
            let profile = directory.join("profile");
            fs::create_dir_all(project.join("src")).unwrap();
            fs::create_dir_all(profile.join("durable")).unwrap();
            fs::write(
                project.join("src/example.ts"),
                b"export const example = 1;\n",
            )
            .unwrap();
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
                .expect("coordinator must rebuild the acp-fixture executable");
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
                Arc::new(BindingRegistry::desktop_pi_fixture(selection.clone()).unwrap());
            let database_path = profile.join("durable/sage.db");
            let mut setup = FixtureSetup::open_with_registry_at_root(
                &database_path,
                Some(database_path.clone()),
                registry,
                &project,
            )
            .await;
            setup.chat_id = setup.create_chat();
            let initial_page = setup
                .service
                .load_chat_page(setup.project_id.clone(), setup.chat_id.clone(), None)
                .unwrap();
            assert!(
                initial_page.turns.as_slice().is_empty(),
                "native Pi cases must start without seeded Turns"
            );
            let root = || {
                BindingProjectRoot::Pi(
                    ProjectRootAuthority::for_test(
                        setup.project_id.clone(),
                        ProjectGeneration::ZERO,
                        setup.root_id.clone(),
                        &setup.root_path,
                    )
                    .unwrap(),
                )
            };
            let mut start =
                setup.start_command_for(&setup.chat_id, initial_page.projection_revision);
            start.binding_id = BindingId::new("bindingId", "pi-rpc").unwrap();
            let started = setup
                .service
                .start_session(
                    setup.service.binding_registry().resolve("pi-rpc").unwrap(),
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
                    setup.service.binding_registry().resolve("pi-rpc").unwrap(),
                    setup.activate_command(&owner),
                    root(),
                )
                .await
                .unwrap();
            setup.wait_for_ready(&owner);
            send(&setup, &owner, "first");
            if case == "pi-compaction-started" {
                setup.wait_until(|| {
                    session_items(&setup, &owner)
                        .iter()
                        .any(|item| item.kind == "compactionNotice" && item.lifecycle == "started")
                });
                assert_eq!(
                    setup.completed_turns(&owner).len(),
                    0,
                    "agent_end must not settle a continued Pi turn"
                );
                let turn_id = setup.wait_for_running_turn(&owner);
                setup
                    .service
                    .cancel_session_turn(HostCancelIntent {
                        owner: owner.clone(),
                        turn_id,
                        client_intent_id: ClientIntentId::new(),
                        now_millis: sage_core::time::now_millis(),
                    })
                    .unwrap();
                setup.wait_for_cancelled(&owner);
            } else if case == "pi-non-read-denial" {
                setup.wait_for_cancelled(&owner);
                assert!(
                    session_items(&setup, &owner)
                        .iter()
                        .any(|item| item.kind == "toolCall" && item.lifecycle == "cancelled")
                );
            } else {
                setup.wait_for_completed_after(&owner, &[]);
                if case == "pi-turn-retry" {
                    assert_eq!(
                        setup.page(&owner).turns.as_slice().len(),
                        1,
                        "automatic retry must preserve the original durable Turn"
                    );
                    assert_eq!(setup.completed_turns(&owner).len(), 1);
                    let page = setup.page(&owner);
                    let turn = &page.turns.as_slice()[0];
                    assert_eq!(turn.session_id, owner.session_id);
                    let assistant_attempts = turn
                        .items
                        .as_slice()
                        .iter()
                        .filter(|item| {
                            matches!(
                                &item.content,
                                ItemContent::Text {
                                    role: TextRole::Assistant,
                                    ..
                                }
                            )
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(
                        assistant_attempts.len(),
                        2,
                        "both assistant attempts persist under the same Turn"
                    );
                    assert_eq!(
                        assistant_attempts
                            .iter()
                            .map(|item| item.lifecycle)
                            .collect::<Vec<_>>(),
                        [ItemLifecycle::Failed, ItemLifecycle::Completed]
                    );
                    assert_ne!(assistant_attempts[0].id, assistant_attempts[1].id);
                    assert!(
                        matches!(&assistant_attempts[0].content, ItemContent::Text { text, .. }
                        if text.as_str() == "Synthetic replay.\n\nThe synthetic example is available for native replay review.\n\nSynthetic replay completed.")
                    );
                    assert!(
                        matches!(&assistant_attempts[1].content, ItemContent::Text { text, .. }
                        if text.as_str() == "Synthetic retry completed.")
                    );
                }
                if case == "pi-idle-config-journey" {
                    let completed = setup.completed_turns(&owner);
                    set_config(&setup, &owner, "pi.thinking", "high");
                    set_config(&setup, &owner, "pi.model", "openai:fixture-model-two");
                    assert!(
                        setup.service.lane_at(0).runtime.is_active(),
                        "idle config must not tear down the session"
                    );
                    send(&setup, &owner, "second");
                    setup.wait_for_completed_after(&owner, &completed);
                    assert_eq!(setup.completed_turns(&owner).len(), 2);
                    setup.wait_until(|| {
                        setup
                            .service
                            .lane_at(0)
                            .runtime
                            .config
                            .live_snapshot(&owner)
                            .is_some_and(|overlay| {
                                overlay
                                    .config
                                    .config_options
                                    .as_slice()
                                    .iter()
                                    .any(|option| option.id.as_str() == "pi.thinking")
                            })
                    });
                }
                if case.starts_with("pi-compaction-") {
                    let lifecycle = case.strip_prefix("pi-compaction-").unwrap();
                    assert!(
                        session_items(&setup, &owner)
                            .iter()
                            .any(|item| item.kind == "compactionNotice"
                                && item.lifecycle == lifecycle)
                    );
                }
                if case == "pi-cancelled-dialog" {
                    assert!(
                        session_items(&setup, &owner)
                            .iter()
                            .any(|item| item.kind == "unsupported")
                    );
                }
            }
            let before = setup.page(&owner);
            setup.service.shutdown_blocking().unwrap();
            setup.wait_for_job_zero();
            drop(setup);
            assert!(selection.consume_one_shot().is_err());
            let registry = Arc::new(BindingRegistry::desktop_pi_fixture(selection).unwrap());
            let reopened = AcpService::new_with_registry_for_test(
                crate::database::open_database(&database_path).unwrap(),
                registry,
            )
            .unwrap();
            let restored = reopened
                .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
                .unwrap();
            assert_eq!(restored.turns, before.turns);
            let session = restored
                .sessions
                .as_slice()
                .iter()
                .find(|session| session.id == owner.session_id)
                .unwrap();
            assert_eq!(session.binding_id.as_str(), "pi-rpc");
            assert_eq!(
                session.residency,
                sage_acp_protocol::lifecycle::Residency::Cold
            );
            let external: String = reopened
                .database
                .connection()
                .query_row(
                    "SELECT external_session_id FROM agent_sessions WHERE id=?1",
                    [owner.session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(external, "2026-01-01T00-00-00-000Z_fixture-pi-session");
            reopened.shutdown_blocking().unwrap();
            drop(reopened);
            fs::remove_dir_all(directory).unwrap();
        }
    });
}

fn send(setup: &FixtureSetup, owner: &RuntimeOwner, id: &str) {
    let mut command = setup.turn_command(owner);
    command.prompt = Prompt::new("prompt", format!("Inspect the synthetic {id} example.")).unwrap();
    assert!(matches!(
        setup
            .service
            .send_session_turn(HostPromptIntent {
                command,
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .unwrap(),
        HostOutcome::PromptAccepted
    ));
}

fn set_config(setup: &FixtureSetup, owner: &RuntimeOwner, id: &str, value: &str) {
    let overlay = setup
        .service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)
        .unwrap();
    setup
        .service
        .configure_session(ConfigIntent {
            owner: owner.clone(),
            expected_config_revision: overlay.config_revision,
            mutation: ConfigMutation::Option {
                config_id: ConfigId::new("configId", id).unwrap(),
                value: ConfigValue::Select {
                    value: ConfigId::new("value", value).unwrap(),
                },
            },
        })
        .unwrap();
    setup.wait_until(|| {
        setup
            .service
            .lane_at(0)
            .runtime
            .config
            .live_snapshot(owner)
            .is_some_and(|current| current.config_revision != overlay.config_revision)
    });
}
