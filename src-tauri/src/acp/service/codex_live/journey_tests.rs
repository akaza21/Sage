use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use super::*;
use crate::{
    acp::{
        bindings::codex::{
            CodexAdmissionUnavailable, CodexBindingRegistration, CodexProjectRootAuthority,
        },
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageChatId,
            SageProjectId, SageRootId, SageTurnId,
        },
        intent::{BindingId, Prompt},
        lifecycle::{ItemLifecycle, SessionState, ToolState, TurnState},
        projection::{ItemContent, ToolPresentation},
        runtime::{
            HostCancelIntent, ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher,
        },
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, StartSessionCommand,
        },
    },
    domain::ProjectLayoutRequest,
    project_registry::ProjectRootAuthority,
};

#[test]
#[cfg(windows)]
fn unavailable_binding_cannot_start_a_live_codex_session() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup = FixtureSetup::in_memory(CodexBindingRegistration::unavailable_for_test()).await;

        assert!(matches!(
            setup
                .service
                .start_codex_session(&setup.root(), setup.start_command())
                .await,
            Err(CodexSessionError::Binding(_))
        ));
        assert!(matches!(
            setup
                .service
                .send_codex_turn(
                    setup.turn_command(&RuntimeOwner {
                        project_id: setup.project_id.clone(),
                        chat_id: setup.chat_id.clone(),
                        session_id: crate::acp::contract::SageAgentSessionId::parse(
                            "session-unavailable"
                        )
                        .unwrap(),
                        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    })
                ),
            Err(CodexSessionError::Intent)
        ));
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn two_sequential_turns_share_one_session_and_converge_to_zero() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;

        let first = setup.send_and_wait_completed(&owner).await;
        let second = setup.send_and_wait_completed(&owner).await;
        assert_ne!(first, second);
        assert_completed_turns(&setup, &owner, &[first, second]);

        setup.close(&owner);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn two_chats_are_sequential_on_the_one_runtime_slot() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
                "characterization-two-turn-success",
                "characterization-resume-turn-success",
            ]))
            .await;
        let owner_a = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let chat_b = setup.create_chat();
        let owner_b = setup
            .start_session_for_chat(chat_b, RuntimeEpoch::new(1).unwrap())
            .await;

        assert!(matches!(
            setup.activate(&owner_b).await,
            Err(CodexSessionError::Capacity)
        ));
        let turns_a = [
            setup.send_and_wait_completed(&owner_a).await,
            setup.send_and_wait_completed(&owner_a).await,
        ];

        setup.activate(&owner_b).await.unwrap();
        setup.wait_for_ready(&owner_b);
        let parked_a = setup.page(&owner_a);
        let parked_a_session = parked_a
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == owner_a.session_id)
            .unwrap();
        assert_eq!(parked_a_session.state, SessionState::Ready);
        assert_eq!(
            parked_a_session.residency,
            crate::acp::lifecycle::Residency::Cold
        );
        assert_eq!(
            parked_a_session.next_runtime_epoch,
            Some(RuntimeEpoch::new(2).unwrap())
        );
        let turns_b = [
            setup.send_and_wait_completed(&owner_b).await,
            setup.send_and_wait_completed(&owner_b).await,
        ];
        assert_completed_turns(&setup, &owner_b, &turns_b);
        assert_completed_turns(&setup, &owner_a, &turns_a);

        setup.close(&owner_b);
        assert_job_zero();
        let resumed_a = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner_a.clone()
        };
        setup.activate(&resumed_a).await.unwrap();
        setup.wait_for_ready(&resumed_a);
        let resumed_turn = setup.send_and_wait_completed(&resumed_a).await;
        assert_completed_turns(
            &setup,
            &resumed_a,
            &[turns_a[0].clone(), turns_a[1].clone(), resumed_turn],
        );
        setup.close(&resumed_a);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn failed_replacement_park_retains_recovery_and_retries_without_a_second_tree() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup = FixtureSetup::in_memory_with_shutdown_phase_failure(
            CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
                "characterization-two-turn-success",
            ]),
        )
        .await;
        let owner_a = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let completed = setup.send_and_wait_completed(&owner_a).await;
        let owner_b = setup
            .start_session_for_chat(setup.create_chat(), RuntimeEpoch::new(1).unwrap())
            .await;

        assert!(matches!(
            setup.activate(&owner_b).await,
            Err(CodexSessionError::Recovery)
        ));
        assert!(
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0
                <= 1
        );
        assert!(setup.service.has_retained_live_host_recovery_for(&owner_a));
        assert_completed_turns(&setup, &owner_a, &[completed]);

        setup.activate(&owner_b).await.unwrap();
        setup.wait_for_ready(&owner_b);
        let session = setup
            .page(&owner_a)
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == owner_a.session_id)
            .unwrap()
            .clone();
        assert_eq!(session.state, SessionState::Ready);
        assert_eq!(session.residency, crate::acp::lifecycle::Residency::Cold);
        assert_eq!(
            session.next_runtime_epoch,
            Some(RuntimeEpoch::new(2).unwrap())
        );
        setup.send_and_wait_completed(&owner_b).await;
        setup.close(&owner_b);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn restart_resume_then_another_turn_reads_sqlite() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let registration = CodexBindingRegistration::admitted_for_journey_test(&[
            "codex-journey-turn-success",
            "characterization-resume-turn-success",
        ]);
        let setup = FixtureSetup::file_backed(registration.clone()).await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let first = setup.send_and_wait_completed(&owner).await;
        let database_path = setup.database_path.clone().unwrap();
        let project_id = setup.project_id.clone();
        let chat_id = setup.chat_id.clone();
        let root_path = setup.root_path.clone();
        let root_id = setup.root_id.clone();

        let shutdown_started = Instant::now();
        setup.service.shutdown_blocking().unwrap_or_else(|error| {
            panic!(
                "restart first-life shutdown failed: {error:?} after {} in {}ms",
                setup.service.last_shutdown_stage(),
                shutdown_started.elapsed().as_millis()
            )
        });
        drop(setup);
        assert_job_zero();

        let database = crate::persistence::Database::open(&database_path).unwrap();
        let resumed = AcpService::new_with_codex_publisher_for_test(
            database,
            registration,
            std::sync::Arc::new(JourneyPublisher),
        )
        .unwrap();
        let page = resumed
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        assert!(
            page.sessions
                .as_slice()
                .iter()
                .any(|session| session.id == owner.session_id)
        );
        let resumed_owner = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner.clone()
        };
        let resumed = FixtureSetup::reopened(
            resumed,
            database_path.clone(),
            project_id,
            chat_id,
            root_path,
            root_id,
        );

        resumed.activate(&resumed_owner).await.unwrap();
        resumed.wait_for_ready(&resumed_owner);
        let second = resumed.send_and_wait_completed(&resumed_owner).await;
        assert_completed_turns(&resumed, &resumed_owner, &[first, second]);
        resumed.close(&resumed_owner);
        assert_job_zero();
        resumed.service.shutdown_blocking().unwrap();
        drop(resumed);
        let _ = std::fs::remove_dir_all(database_path.parent().unwrap());
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn cancel_then_reuse_session_converges_to_zero_orphans() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-cancel-reuse",
                "characterization-resume-turn-success",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;

        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        let turn_id = setup.wait_for_running_turn(&owner);
        setup
            .service
            .cancel_codex_turn(HostCancelIntent {
                owner: owner.clone(),
                turn_id: turn_id.clone(),
                client_intent_id: ClientIntentId::new(),
                now_millis: crate::persistence::now_millis(),
            })
            .unwrap_or_else(|error| {
                panic!(
                    "cancel seam failed: {error:?} after {}; turns={:?}",
                    setup.service.last_cancel_stage(),
                    setup
                        .page(&owner)
                        .turns
                        .as_slice()
                        .iter()
                        .map(|turn| (turn.id.as_str().to_owned(), format!("{:?}", turn.state)))
                        .collect::<Vec<_>>()
                )
            });
        setup.wait_for_cancelled(&owner);

        // The production cancel path must relinquish its Job before this
        // session is activated again. A direct slot mutation would not prove it.
        setup.wait_for_job_zero();
        let reuse_owner = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner.clone()
        };
        setup.activate(&reuse_owner).await.unwrap();
        setup.wait_for_ready(&reuse_owner);
        setup.send_and_wait_completed(&reuse_owner).await;
        setup.close(&reuse_owner);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap_or_else(|error| {
            panic!(
                "cancel final shutdown failed: {error:?} after {}",
                setup.service.last_shutdown_stage()
            )
        });
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn child_exit_after_item_completion_fails_the_turn_and_ends_the_runtime_epoch() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "codex-item-completed-then-exit",
                "codex-item-completed-then-exit",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;

        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        setup.wait_until(|| {
            let page = setup.page(&owner);
            page.turns
                .as_slice()
                .iter()
                .any(|turn| turn.session_id == owner.session_id && turn.state.is_terminal())
                && page.sessions.as_slice().iter().any(|session| {
                    session.id == owner.session_id && session.state == SessionState::Failed
                })
        });

        let owner_turns: i64 = setup
            .service
            .database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE session_id=?1",
                rusqlite::params![owner.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owner_turns, 1);

        let (turn_id, turn_state, delivery_state): (String, String, String) = setup
            .service
            .database
            .connection()
            .query_row(
                "SELECT id, state, delivery_state FROM turns WHERE session_id=?1",
                rusqlite::params![owner.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (turn_state.as_str(), delivery_state.as_str()),
            ("failed", "correlated")
        );

        let items = session_items(&setup, &owner);
        let harness_completed = items
            .iter()
            .filter(|item| item.lifecycle == "completed" && !item.external_item_id.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(harness_completed.len(), 1, "items={items:?}");
        assert_eq!(
            (
                harness_completed[0].kind.as_str(),
                harness_completed[0].lifecycle.as_str(),
                harness_completed[0].tool_state.as_str(),
                harness_completed[0].turn_id.as_str(),
                harness_completed[0].external_item_id.as_str(),
            ),
            (
                "toolCall",
                "completed",
                "completed",
                turn_id.as_str(),
                "fixture-command-completed",
            ),
            "items={items:?}"
        );

        let page = setup.page(&owner);
        let history_turn = page
            .turns
            .as_slice()
            .iter()
            .find(|history_turn| history_turn.id.as_str() == turn_id.as_str())
            .unwrap();
        let history_item = history_turn
            .items
            .as_slice()
            .iter()
            .find(|item| matches!(&item.content, ItemContent::ToolCall { .. }))
            .unwrap();
        assert_eq!(history_item.lifecycle, ItemLifecycle::Completed);
        assert_eq!(history_item.tool_state, Some(ToolState::Completed));
        let ItemContent::ToolCall {
            presentation: ToolPresentation::Execute { label, .. },
            input_summary: Some(input_summary),
            output_summary: Some(output_summary),
        } = &history_item.content
        else {
            panic!("expected completed fixture command in history");
        };
        assert_eq!(label.as_str(), "echo fixture-command-completed");
        assert_eq!(input_summary.as_str(), "echo fixture-command-completed");
        assert_eq!(output_summary.as_str(), "fixture command completed");

        let runtime_state: String = setup
            .service
            .database
            .connection()
            .query_row(
                "SELECT state FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
                rusqlite::params![owner.session_id.as_str(), owner.runtime_epoch.get() as i64],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(runtime_state, "failed");

        assert_job_zero();
        let replacement = setup
            .start_session_for_chat(setup.chat_id.clone(), RuntimeEpoch::new(1).unwrap())
            .await;
        setup.activate(&replacement).await.unwrap();
        setup.wait_for_ready(&replacement);
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn lost_steer_result_keeps_an_outcome_unknown_queue_row_after_reopen() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup = FixtureSetup::file_backed(
            CodexBindingRegistration::admitted_for_observed_steer_journey_test(&[
                "codex-steer-lost-result",
            ]),
        )
        .await;
        assert_lost_steer_result_after_reopen(setup, None, None).await;
    });
}

#[test]
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
fn desktop_codex_fixture_lost_steer_result_survives_file_backed_reopen() {
    use sha2::{Digest, Sha256};

    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let proof_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("work/cp-native-proof");
        std::fs::create_dir_all(&proof_root).unwrap();
        let profile_root = proof_root.join(format!("cp14-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&profile_root).unwrap();
        std::fs::write(
            profile_root.join(".sage-disposable-profile-v1"),
            b"sage-disposable-profile-v1\n",
        )
        .unwrap();
        let artifact_root =
            std::env::temp_dir().join(format!("sage-cp14-artifact-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&artifact_root).unwrap();
        let artifact = artifact_root.join("acp-adversarial-fixture.exe");
        let source = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("acp-adversarial-fixture.exe");
        std::fs::copy(source, &artifact).unwrap();
        let bytes = std::fs::metadata(&artifact).unwrap().len().to_string();
        let digest = format!("{:x}", Sha256::digest(std::fs::read(&artifact).unwrap()));
        let project_root = std::env::current_dir().unwrap();
        let profile = crate::launch_profile::resolve_launch_profile(
            Some(profile_root.as_os_str()),
            Some(project_root.as_os_str()),
            std::env::temp_dir().join("sage-cp14-normal-durable"),
            std::env::temp_dir().join("sage-cp14-normal-webview"),
        )
        .unwrap();
        let selection = crate::launch_profile::resolve_codex_fixture_selection(
            &profile,
            Some(artifact.as_os_str()),
            Some(std::ffi::OsStr::new(&digest)),
            Some(std::ffi::OsStr::new(&bytes)),
            Some(std::ffi::OsStr::new("codex-steer-lost-result")),
            &proof_root,
        )
        .unwrap()
        .unwrap();
        let receipt = selection.durable_dir.join("codex-fixture-launch-once-v1");
        let registry = std::sync::Arc::new(
            crate::acp::bindings::registry::BindingRegistry::desktop_codex_fixture(
                selection.clone(),
            )
            .unwrap(),
        );
        let database_path = selection.durable_dir.join("sage.db");
        let selected_project_root = selection.project_root.clone();
        let setup = FixtureSetup::open_with_registry_at_root(
            &database_path,
            Some(database_path.clone()),
            registry,
            &selected_project_root,
        )
        .await;
        assert!(
            !receipt.exists(),
            "catalog and session start did not launch"
        );
        let reopen_registry = Box::new(move || {
            std::sync::Arc::new(
                crate::acp::bindings::registry::BindingRegistry::desktop_codex_fixture(selection)
                    .unwrap(),
            )
        });
        assert_lost_steer_result_after_reopen(setup, Some(reopen_registry), Some(receipt)).await;
        std::fs::remove_dir_all(&profile_root).unwrap();
        std::fs::remove_dir_all(&artifact_root).unwrap();
    });
}

#[cfg(windows)]
async fn assert_lost_steer_result_after_reopen(
    setup: FixtureSetup,
    #[allow(unused_variables)] reopen_registry: Option<
        Box<dyn FnOnce() -> std::sync::Arc<crate::acp::bindings::registry::BindingRegistry>>,
    >,
    fixture_receipt: Option<PathBuf>,
) {
    use crate::acp::runtime::HostSteerIntent;
    use crate::chat_queue::{DurableQueuedPrompt, QueuedPromptStatus};

    let owner = if let Some(receipt) = &fixture_receipt {
        let owner = setup
            .start_session_for_chat(setup.chat_id.clone(), RuntimeEpoch::new(1).unwrap())
            .await;
        assert!(
            !receipt.exists(),
            "Session start must not consume fixture case"
        );
        setup.activate(&owner).await.unwrap();
        setup.wait_for_ready(&owner);
        owner
    } else {
        setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await
    };
    if let Some(receipt) = &fixture_receipt {
        assert!(
            receipt.is_file(),
            "Session activation consumed the fixture case"
        );
    }
    setup
        .service
        .send_codex_turn(setup.turn_command(&owner))
        .unwrap();
    let turn_id = setup.wait_for_running_turn(&owner);
    let queued = DurableQueuedPrompt {
        id: "same-message-id".into(),
        text: "Change direction".into(),
        attached_context: Default::default(),
        binding_id: "codex-app-server".into(),
        model_id: None,
        status: QueuedPromptStatus::Sending,
    };
    setup
        .service
        .database
        .replace_chat_queued_prompts(
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            std::slice::from_ref(&queued),
        )
        .unwrap();
    let outcome = setup.service.dispatch_session_steer(
        &BindingId::new("bindingId", "codex-app-server").unwrap(),
        HostSteerIntent {
            owner: owner.clone(),
            turn_id,
            prompt: Prompt::new("prompt", &queued.text).unwrap(),
            client_message_id: crate::acp::contract::RequiredText::new(
                "clientMessageId",
                &queued.id,
            )
            .unwrap(),
        },
    );
    match outcome {
        Err(crate::acp::service::SessionDispatchError::Operation(
            CodexSessionError::SteerOutcomeUnknown,
        )) => {}
        Err(crate::acp::service::SessionDispatchError::Operation(error)) => {
            panic!("lost Steer result was classified {error:?}")
        }
        Err(crate::acp::service::SessionDispatchError::Binding) => {
            panic!("observed fixture did not advertise Steer")
        }
        Ok(_) => panic!("lost Steer result was acknowledged"),
    }
    setup
        .service
        .database
        .replace_chat_queued_prompts(
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            &[DurableQueuedPrompt {
                status: QueuedPromptStatus::OutcomeUnknown,
                ..queued
            }],
        )
        .unwrap();
    setup.wait_for_job_zero();
    let turns_before_reopen: i64 = setup
        .service
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM turns WHERE chat_id=?1",
            [owner.chat_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let database_path = setup.database_path.clone().unwrap();
    let project_id = owner.project_id.as_str().to_owned();
    let chat_id = owner.chat_id.as_str().to_owned();
    if fixture_receipt.is_some() {
        setup.service.shutdown_blocking().unwrap();
    } else {
        let _ = setup.service.shutdown_blocking();
    }
    drop(setup);

    let reopened = crate::persistence::Database::open(&database_path).unwrap();
    let restored = reopened.project_chat_queued_prompts(&project_id).unwrap();
    let row = &restored[&chat_id][0];
    assert_eq!(row.id, "same-message-id");
    assert!(matches!(row.status, QueuedPromptStatus::OutcomeUnknown));
    let turns: i64 = reopened
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM turns WHERE chat_id=?1",
            [&chat_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        turns, turns_before_reopen,
        "restoring the queue must not dispatch a Turn"
    );
    assert_job_zero();
    #[cfg(all(feature = "acp-fixture", debug_assertions))]
    if let Some(build_registry) = reopen_registry {
        let registry = build_registry();
        let service = AcpService::new_with_registry_for_test(reopened.clone(), registry).unwrap();
        let page = service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap();
        assert_eq!(page.chat_id, owner.chat_id);
        let queue = service
            .database
            .project_chat_queued_prompts(&project_id)
            .unwrap();
        assert_eq!(queue[&chat_id].len(), 1);
        assert!(matches!(
            queue[&chat_id][0].status,
            QueuedPromptStatus::OutcomeUnknown
        ));
        assert_job_zero();
        service.shutdown_blocking().unwrap();
    }
    drop(reopened);
    std::fs::remove_dir_all(database_path.parent().unwrap()).unwrap();
}

#[test]
#[cfg(windows)]
fn unsigned_or_observation_only_cannot_launch() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        for (registration, unavailable) in [
            (
                CodexBindingRegistration::unsigned_for_test(),
                CodexAdmissionUnavailable::IndependentAdmission,
            ),
            (
                CodexBindingRegistration::observation_only_for_test(),
                CodexAdmissionUnavailable::ObservationOnly,
            ),
        ] {
            let setup = FixtureSetup::in_memory(registration).await;
            assert!(matches!(
                setup.service.start_codex_session(&setup.root(), setup.start_command()).await,
                Err(CodexSessionError::Binding(reason)) if reason == unavailable
            ));
            assert_job_zero();
            setup.service.shutdown_blocking().unwrap();
        }
    });
}

#[cfg(windows)]
struct FixtureSetup {
    service: AcpService,
    project_id: SageProjectId,
    chat_id: SageChatId,
    root_id: SageRootId,
    root_path: PathBuf,
    database_path: Option<PathBuf>,
}

#[cfg(windows)]
impl FixtureSetup {
    async fn in_memory(registration: CodexBindingRegistration) -> Self {
        Self::open(Path::new(":memory:"), None, registration).await
    }

    async fn in_memory_with_shutdown_phase_failure(registration: CodexBindingRegistration) -> Self {
        let setup = Self::open(Path::new(":memory:"), None, registration).await;
        setup.service.fail_next_runtime_port_finish_park();
        setup
    }

    async fn file_backed(registration: CodexBindingRegistration) -> Self {
        let directory =
            std::env::temp_dir().join(format!("sage-codex-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        Self::open(&path, Some(path.clone()), registration).await
    }

    async fn open(
        path: &Path,
        database_path: Option<PathBuf>,
        registration: CodexBindingRegistration,
    ) -> Self {
        Self::open_with_registry(
            path,
            database_path,
            std::sync::Arc::new(crate::acp::bindings::registry::BindingRegistry::with_codex(
                registration,
            )),
        )
        .await
    }

    async fn open_with_registry(
        path: &Path,
        database_path: Option<PathBuf>,
        registry: std::sync::Arc<crate::acp::bindings::registry::BindingRegistry>,
    ) -> Self {
        let project_root = std::env::current_dir().unwrap();
        Self::open_with_registry_at_root(path, database_path, registry, &project_root).await
    }

    async fn open_with_registry_at_root(
        path: &Path,
        database_path: Option<PathBuf>,
        registry: std::sync::Arc<crate::acp::bindings::registry::BindingRegistry>,
        selected_project_root: &Path,
    ) -> Self {
        let database = crate::persistence::Database::open(path).unwrap();
        let root_path = selected_project_root.canonicalize().unwrap();
        let project = database.create_project_from_root(&root_path).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        Self {
            service: AcpService::new_with_registry_publisher_for_test(
                database,
                registry,
                std::sync::Arc::new(JourneyPublisher),
            )
            .unwrap(),
            project_id: SageProjectId::parse(project.id).unwrap(),
            chat_id: SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap(),
            root_id: SageRootId::parse(project.roots[0].id.clone()).unwrap(),
            root_path,
            database_path,
        }
    }

    fn reopened(
        service: AcpService,
        database_path: PathBuf,
        project_id: SageProjectId,
        chat_id: SageChatId,
        root_path: PathBuf,
        root_id: SageRootId,
    ) -> Self {
        Self {
            service,
            project_id,
            chat_id,
            root_id,
            root_path,
            database_path: Some(database_path),
        }
    }

    fn root(&self) -> CodexProjectRootAuthority {
        CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                self.project_id.clone(),
                ProjectGeneration::ZERO,
                self.root_id.clone(),
                &self.root_path,
            )
            .unwrap(),
        )
    }

    async fn start_and_activate(&self, epoch: RuntimeEpoch) -> RuntimeOwner {
        let owner = self
            .start_session_for_chat(self.chat_id.clone(), epoch)
            .await;
        self.activate(&owner).await.unwrap();
        self.wait_for_ready(&owner);
        owner
    }

    async fn start_session_for_chat(
        &self,
        chat_id: SageChatId,
        epoch: RuntimeEpoch,
    ) -> RuntimeOwner {
        crate::acp::service::tests::seed_codex_discovery(&self.service, &self.root(), None);
        let revision = self
            .service
            .load_chat_page(self.project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = self
            .service
            .start_codex_session(&self.root(), self.start_command_for(&chat_id, revision))
            .await
            .unwrap();
        RuntimeOwner {
            project_id: self.project_id.clone(),
            chat_id,
            session_id: started.session_id,
            runtime_epoch: epoch,
        }
    }

    async fn activate(&self, owner: &RuntimeOwner) -> Result<(), CodexSessionError> {
        self.service
            .activate_codex_session(self.activate_command(owner), self.root())
            .await
    }

    async fn send_and_wait_completed(&self, owner: &RuntimeOwner) -> SageTurnId {
        let completed_before = self.completed_turns(owner);
        self.service
            .send_codex_turn(self.turn_command(owner))
            .unwrap();
        self.wait_for_completed_after(owner, &completed_before)
    }

    fn close(&self, owner: &RuntimeOwner) {
        self.service
            .close_codex_session(self.close_command(owner))
            .unwrap();
    }

    fn create_chat(&self) -> SageChatId {
        let snapshot = self.service.database.bootstrap_snapshot().unwrap();
        SageChatId::parse(
            self.service
                .database
                .create_chat(&ProjectLayoutRequest {
                    project_id: self.project_id.as_str().to_owned(),
                    tabs: snapshot.tabs,
                    active_tab_id: snapshot.active_tab_id,
                    selected_root_id: snapshot.selected_root_id,
                    selected_resource: snapshot.selected_resource,
                })
                .unwrap(),
        )
        .unwrap()
    }

    fn start_command(&self) -> StartSessionCommand {
        self.start_command_for(&self.chat_id, ProjectionRevision::ZERO)
    }

    fn start_command_for(
        &self,
        chat_id: &SageChatId,
        revision: ProjectionRevision,
    ) -> StartSessionCommand {
        StartSessionCommand {
            project_id: self.project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            binding_digest: "forged".into(),
            harness_version: None,
            model_id: None,
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn activate_command(&self, owner: &RuntimeOwner) -> ActivateSessionCommand {
        ActivateSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "forged".into(),
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn turn_command(&self, owner: &RuntimeOwner) -> PrepareTurnCommand {
        PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            prompt: Prompt::new("prompt", "Complete this short test turn.").unwrap(),
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn close_command(&self, owner: &RuntimeOwner) -> CloseSessionCommand {
        CloseSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn page(&self, owner: &RuntimeOwner) -> crate::acp::projection::AcpChatPage {
        self.service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
    }

    fn wait_for_ready(&self, owner: &RuntimeOwner) {
        self.wait_until(|| {
            self.page(owner).sessions.as_slice().iter().any(|session| {
                session.id == owner.session_id && session.state == SessionState::Ready
            })
        });
    }

    fn wait_for_completed_after(
        &self,
        owner: &RuntimeOwner,
        completed_before: &[SageTurnId],
    ) -> SageTurnId {
        self.wait_until(|| {
            self.completed_turns(owner)
                .iter()
                .any(|turn_id| !completed_before.contains(turn_id))
        });
        self.completed_turns(owner)
            .into_iter()
            .find(|turn_id| !completed_before.contains(turn_id))
            .unwrap()
    }

    fn wait_for_running_turn(&self, owner: &RuntimeOwner) -> SageTurnId {
        self.wait_until(|| {
            self.page(owner)
                .turns
                .as_slice()
                .iter()
                .any(|turn| turn.session_id == owner.session_id && turn.state == TurnState::Running)
        });
        self.page(owner)
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == owner.session_id && turn.state == TurnState::Running)
            .unwrap()
            .id
            .clone()
    }

    fn wait_for_cancelled(&self, owner: &RuntimeOwner) {
        self.wait_until(|| {
            self.page(owner).turns.as_slice().iter().any(|turn| {
                turn.session_id == owner.session_id
                    && matches!(turn.state, TurnState::Cancelled | TurnState::Interrupted)
            })
        });
    }

    fn completed_turns(&self, owner: &RuntimeOwner) -> Vec<SageTurnId> {
        self.page(owner)
            .turns
            .as_slice()
            .iter()
            .filter(|turn| {
                turn.session_id == owner.session_id && turn.state == TurnState::Completed
            })
            .map(|turn| turn.id.clone())
            .collect()
    }

    fn wait_for_job_zero(&self) {
        self.wait_until(|| {
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0
                == 0
        });
    }

    #[track_caller]
    fn wait_until(&self, condition: impl Fn() -> bool) {
        crate::test_wait::until("production Codex journey", condition);
    }
}

#[cfg(windows)]
struct JourneyPublisher;

#[cfg(windows)]
impl ProjectionPublisher for JourneyPublisher {
    fn publish_committed(
        &self,
        event: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        if event.projection_revision == ProjectionRevision::ZERO {
            return Err(ProjectionPublishError::InvalidRevision);
        }
        Ok(ProjectionPublishStatus::Published)
    }
}

#[cfg(windows)]
fn assert_job_zero() {
    assert_eq!(
        crate::acp::process::TreeCapacityPool::global()
            .observation_usage()
            .0,
        0
    );
}

#[cfg(windows)]
#[derive(Debug)]
struct DurableItem {
    kind: String,
    lifecycle: String,
    tool_state: String,
    turn_id: String,
    external_item_id: String,
}

#[cfg(windows)]
fn session_items(setup: &FixtureSetup, owner: &RuntimeOwner) -> Vec<DurableItem> {
    let connection = setup.service.database.connection();
    let mut statement = connection
        .prepare(
            "SELECT kind, lifecycle, COALESCE(tool_state, ''), turn_id, COALESCE(external_item_id, '') FROM items \
             WHERE session_id=?1 ORDER BY ordinal",
        )
        .unwrap();
    let rows = statement
        .query_map(rusqlite::params![owner.session_id.as_str()], |row| {
            Ok(DurableItem {
                kind: row.get(0)?,
                lifecycle: row.get(1)?,
                tool_state: row.get(2)?,
                turn_id: row.get(3)?,
                external_item_id: row.get(4)?,
            })
        })
        .unwrap();
    rows.map(Result::unwrap).collect()
}

#[cfg(windows)]
fn assert_completed_turns(setup: &FixtureSetup, owner: &RuntimeOwner, expected: &[SageTurnId]) {
    let completed = setup.completed_turns(owner);
    assert_eq!(completed.len(), expected.len());
    for turn_id in expected {
        assert!(completed.contains(turn_id));
    }
}

#[cfg(windows)]
#[path = "journey_shutdown_tests.rs"]
mod journey_shutdown_tests;
