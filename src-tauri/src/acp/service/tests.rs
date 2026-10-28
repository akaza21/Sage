use std::{
    fs,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use crate::{
    acp::{
        bindings::{
            claude::ClaudeBindingRegistration,
            codex::CodexBindingRegistration,
            registry::{BindingProjectRoot, BindingRegistry},
        },
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RequiredText, RuntimeEpoch,
            SageAgentSessionId, SageChatId, SageProjectId, SageRootId, SageTurnId,
        },
        intent::{CapabilitySnapshotV1, ExternalId, IntentSnapshot, ProjectionChangedV1, Prompt},
        lifecycle::TurnState,
        runtime::ActivationMode,
        runtime::{
            ActiveTurnIntent, AdapterBatchJournal, DurableAdapterEvent, HostCancelIntent,
            HostCloseIntent, HostError, HostPromptIntent, HostState, JournalLane,
            NotificationError, NotificationSink, NotificationSinkFuture, PrepareDispatch,
            ProjectIntentAuthority, PromptWrite, PublishOutcome, RuntimeEngine, RuntimeHost,
            SendCoordinator, SessionPumpIntent, SqliteAdapterBatchJournal, TurnScope,
            ZeroProcessProof,
        },
        scheduler::RuntimeOwner,
        store::{
            AbortActivationCommand, ActivateSessionCommand, ActivationAbortCategory,
            AdapterTurnCursor, CompleteActivationCommand, ParkSessionCommand, PrepareTurnCommand,
            StartSessionCommand,
        },
    },
    domain::{ProjectLayoutRequest, ProjectSwitchRequest},
    persistence::Database,
    project_registry::ProjectRootAuthority,
};

#[cfg(windows)]
use crate::acp::{bindings::codex::CodexProjectRootAuthority, contract::SafeText};

use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};

use super::AcpService;

#[test]
fn admitted_claude_session_starts_and_prepares_activation_without_codex_admission() {
    tauri::async_runtime::block_on(async {
        let fixture_root =
            std::env::temp_dir().join(format!("sage-claude-activation-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&fixture_root).unwrap();
        let adapter = fixture_root.join("adapter.js");
        let node = fixture_root.join("node.exe");
        fs::write(&adapter, b"adapter").unwrap();
        fs::write(&node, b"node").unwrap();
        let registration = ClaudeBindingRegistration::admitted_for_test(
            ClaudeLaunchDiscovery::Present(Box::new(DiscoveredClaudeLaunch {
                adapter_path: adapter.clone(),
                adapter_version: "0.76.0".into(),
                acp_sdk_version: "1.4.0".into(),
                node_engine: ">=22".into(),
                node_path: node.clone(),
                node_version: "22.18.0".into(),
                adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
                node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
            })),
        );
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project_root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&project_root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let root_authority = ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            SageRootId::parse(project.roots[0].id.clone()).unwrap(),
            &project_root,
        )
        .unwrap();
        let service = AcpService::new_with_registry_for_test(
            database,
            Arc::new(BindingRegistry::new(
                CodexBindingRegistration::unavailable_for_test(),
                registration,
            )),
        )
        .unwrap();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = service
            .start_session(
                service
                    .binding_registry()
                    .resolve("claude-agent-acp")
                    .unwrap(),
                &BindingProjectRoot::Claude(root_authority),
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "claude-agent-acp")
                        .unwrap(),
                    binding_digest: "forged".into(),
                    harness_version: None,
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await
            .unwrap();
        let root_authority = ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            SageRootId::parse(project.roots[0].id.clone()).unwrap(),
            &project_root,
        )
        .unwrap();
        let (_, prepared) = service
            .prepare_session_activation(
                service
                    .binding_registry()
                    .resolve("claude-agent-acp")
                    .unwrap(),
                ActivateSessionCommand {
                    project_id,
                    chat_id,
                    expected_projection_revision: started.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: started.session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    artifact_digest: "forged".into(),
                    now_millis: crate::persistence::now_millis(),
                },
                BindingProjectRoot::Claude(root_authority),
            )
            .unwrap();
        assert!(matches!(
            prepared.intent.mode,
            ActivationMode::New { model: None, .. }
        ));
        assert!(
            prepared.intent.curated_ceiling.prompt,
            "an admitted Claude ACP binding must leave promptability to its initialize offer"
        );
        assert!(
            prepared.intent.sage_policy.prompt,
            "the activation policy must not suppress the admitted ACP prompt capability"
        );
        assert!(prepared.intent.curated_ceiling.resume_session);
        assert!(!prepared.intent.curated_ceiling.load_session);
        assert!(prepared.intent.curated_ceiling.close_session);
        assert!(prepared.intent.curated_ceiling.cancel);
        assert_eq!(prepared.intent.curated_ceiling.model_count, 0);
        service.shutdown_blocking().unwrap();
        fs::remove_dir_all(fixture_root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn codex_drift_transition_invalidates_the_shared_service_and_catalog_registration() {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let service = AcpService::new_with_codex_for_test(database, registration.clone()).unwrap();
    let catalog = crate::harnesses::HarnessCatalog::with_codex_registration_and_presence(
        registration.clone(),
        crate::acp::bindings::codex::CodexHostPresence::Absent,
    )
    .settled_for_test();
    let project_root = CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            SageProjectId::parse("project-drift").unwrap(),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-drift").unwrap(),
            &std::env::current_dir().unwrap(),
        )
        .unwrap(),
    );

    assert!(registration.same_authority(&service.codex_registration()));
    assert_eq!(catalog.definitions().remove(0).install_state, "available");

    registration.induce_drift_for_test();
    assert_eq!(
        catalog.refresh_codex_registration(),
        Err(crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift)
    );
    assert_eq!(
        catalog.definitions().remove(0).install_state,
        "notInstalled"
    );
    assert!(catalog.models("codex").unwrap().is_empty());

    assert!(
        service
            .codex_runtime(
                project_root,
                RuntimeEpoch::new(1).unwrap(),
                SageAgentSessionId::parse("session-drifted-registration").unwrap(),
            )
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn admitted_codex_session_start_persists_the_exact_curated_identity() {
    tauri::async_runtime::block_on(async {
        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(database.clone(), registration).unwrap();
        let project_root = codex_root(&project_id, &project.roots[0].id, &root);
        seed_codex_discovery(&service, &project_root, None);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;

        let client_intent_id = ClientIntentId::new();
        let first = service
            .start_codex_session(
                &project_root,
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: client_intent_id.clone(),
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "codex-app-server")
                        .unwrap(),
                    binding_digest: "forged-digest".into(),
                    harness_version: Some(SafeText::new("version", "forged-version").unwrap()),
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await
            .unwrap();
        let duplicate = service
            .start_codex_session(
                &project_root,
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id,
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "codex-app-server")
                        .unwrap(),
                    binding_digest: "different-forged-digest".into(),
                    harness_version: None,
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await
            .unwrap();
        assert_eq!(duplicate.session_id, first.session_id);

        let identity: (String, String, Option<String>) = database
            .connection()
            .query_row(
                "SELECT binding_id,binding_digest,harness_version FROM agent_sessions
                 WHERE project_id=?1 AND chat_id=?2 AND binding_id='codex-app-server'",
                rusqlite::params![project_id.as_str(), chat_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(identity.0, "codex-app-server");
        assert_eq!(
            identity.1,
            "3bba7d86d2357d8fccd81bdba3d7f3f4278c80014d7e65e085b9a874330d7df4"
        );
        assert_eq!(identity.2.as_deref(), Some("0.146.0"));
        service.shutdown_blocking().unwrap();
    });
}

#[cfg(windows)]
#[test]
fn codex_start_rejects_a_model_without_current_scoped_discovery() {
    tauri::async_runtime::block_on(async {
        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(database.clone(), registration).unwrap();
        let project_root = codex_root(&project_id, &project.roots[0].id, &root);
        seed_codex_discovery(&service, &project_root, None);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;

        let result = service
            .start_codex_session(
                &project_root,
                StartSessionCommand {
                    project_id,
                    chat_id,
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "codex-app-server")
                        .unwrap(),
                    binding_digest: "forged".into(),
                    harness_version: None,
                    model_id: Some(
                        crate::acp::intent::ModelId::new("modelId", "forged-model").unwrap(),
                    ),
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await;
        assert!(matches!(result, Err(super::CodexSessionError::Intent)));
        let sessions: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE binding_id='codex-app-server'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 0);
        service.shutdown_blocking().unwrap();
    });
}

#[cfg(windows)]
#[test]
fn uncharacterised_observed_codex_starts_without_a_model_catalogue() {
    tauri::async_runtime::block_on(async {
        let root = std::env::temp_dir().join(format!(
            "sage-observed-codex-no-model-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("codex.cmd");
        fs::write(&executable, b"@echo off\r\n").unwrap();
        fs::write(root.join("node.exe"), b"node").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();
        registration
            .authenticate_observed(executable, "codex 0.145.0".into())
            .unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(database.clone(), registration).unwrap();
        let project_root = codex_root(&project_id, &project.roots[0].id, &root);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let mut selected_model = codex_start_command(&project_id, &chat_id, revision);
        selected_model.model_id =
            Some(crate::acp::intent::ModelId::new("modelId", "unverified-model").unwrap());
        assert!(matches!(
            service
                .start_codex_session(&project_root, selected_model)
                .await,
            Err(super::CodexSessionError::Intent)
        ));

        let started = service
            .start_codex_session(
                &project_root,
                codex_start_command(&project_id, &chat_id, revision),
            )
            .await
            .unwrap();

        let model: Option<String> = database
            .connection()
            .query_row(
                "SELECT model_id FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(model.is_none());
        service.shutdown_blocking().unwrap();
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn codex_start_rejects_drift_before_any_durable_write() {
    tauri::async_runtime::block_on(async {
        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service =
            AcpService::new_with_codex_for_test(database.clone(), registration.clone()).unwrap();
        let project_root = codex_root(&project_id, &project.roots[0].id, &root);
        seed_codex_discovery(&service, &project_root, None);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        registration.induce_drift_for_test();

        let result = service
            .start_codex_session(
                &project_root,
                StartSessionCommand {
                    project_id,
                    chat_id,
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: crate::acp::intent::BindingId::new("bindingId", "codex-app-server")
                        .unwrap(),
                    binding_digest: "forged".into(),
                    harness_version: Some(SafeText::new("version", "forged").unwrap()),
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await;

        assert!(matches!(
            result,
            Err(super::CodexSessionError::Binding(
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift
            ))
        ));
        let sessions: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE binding_id='codex-app-server'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sessions, 0);
        service.shutdown_blocking().unwrap();
    });
}

#[cfg(windows)]
#[test]
fn codex_activation_intent_uses_new_only_for_an_exact_created_cold_session() {
    tauri::async_runtime::block_on(async {
        let (database, service, project, project_id, chat_id, root) = codex_session_setup();
        let authority = codex_root(&project_id, &project.roots[0].id, &root);
        seed_codex_discovery(&service, &authority, None);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = service
            .start_codex_session(
                &authority,
                codex_start_command(&project_id, &chat_id, revision),
            )
            .await
            .unwrap();
        let authority = codex_root(&project_id, &project.roots[0].id, &root);
        let admitted = service
            .binding_registry()
            .resolve(crate::acp::bindings::codex::CODEX_BINDING_ID)
            .unwrap();
        let (command, prepared) = service
            .prepare_session_activation(
                admitted,
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: started.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: started.session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    artifact_digest: "forged".into(),
                    now_millis: crate::persistence::now_millis(),
                },
                crate::acp::bindings::registry::BindingProjectRoot::Codex(authority),
            )
            .unwrap();

        assert!(matches!(
            prepared.intent.mode,
            ActivationMode::New { model: None, .. }
        ));
        assert_eq!(command.artifact_digest, prepared.intent.artifact_digest);
        assert_ne!(command.artifact_digest, "forged");
        drop(prepared);
        service.shutdown_blocking().unwrap();
        drop(database);
    });
}

#[cfg(windows)]
#[test]
fn codex_activation_intent_resumes_only_the_exact_durable_external_session() {
    tauri::async_runtime::block_on(async {
        let (_database, service, project, project_id, chat_id, root) = codex_session_setup();
        let authority = codex_root(&project_id, &project.roots[0].id, &root);
        seed_codex_discovery(&service, &authority, None);
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = service
            .start_codex_session(
                &authority,
                codex_start_command(&project_id, &chat_id, revision),
            )
            .await
            .unwrap();
        let binding = service.codex_registration().refresh().unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let mut lease = service
            .lane_at(0)
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                artifact_digest: binding.artifact_digest().to_owned(),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        lease
            .complete_activation(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                binding_digest: binding.binding_digest_hex().to_owned(),
                artifact_digest: binding.artifact_digest().to_owned(),
                external_session_id: ExternalId::new("externalSessionId", "thread-durable-1")
                    .unwrap(),
                capabilities_json: serde_json::to_string(&binding.capability_ceiling().unwrap())
                    .unwrap(),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        lease
            .park_after_zero_process(
                ParkSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    exit_kind: RequiredText::new("exitKind", "testIdle").unwrap(),
                    now_millis: crate::persistence::now_millis(),
                },
                ZeroProcessProof::confirmed(owner),
            )
            .await
            .unwrap();
        let authority = codex_root(&project_id, &project.roots[0].id, &root);
        let admitted = service
            .binding_registry()
            .resolve(crate::acp::bindings::codex::CODEX_BINDING_ID)
            .unwrap();
        let (_, prepared) = service
            .prepare_session_activation(
                admitted,
                ActivateSessionCommand {
                    project_id,
                    chat_id,
                    expected_projection_revision: started.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: started.session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    artifact_digest: "forged".into(),
                    now_millis: crate::persistence::now_millis(),
                },
                crate::acp::bindings::registry::BindingProjectRoot::Codex(authority),
            )
            .unwrap();

        match &prepared.intent.mode {
            ActivationMode::Resume {
                external_session_id,
                model: None,
                ..
            } => {
                assert_eq!(external_session_id.as_str(), "thread-durable-1");
            }
            _ => panic!("durable ready/cold session must resume"),
        }
        drop(prepared);
        service.shutdown_blocking().unwrap();
    });
}

#[cfg(windows)]
#[test]
fn earlier_codex_session_keeps_its_chat_page_and_refuses_native_resume() {
    tauri::async_runtime::block_on(async {
        use sha2::Digest;

        let (database, service, project, project_id, chat_id, root) = codex_session_setup();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let mut old = codex_start_command(&project_id, &chat_id, revision);
        old.binding_digest =
            "f8c2b995be263fe4bf00974043312673378081734ca8c98155d2a33887444b9e".into();
        old.harness_version = Some(SafeText::new("harnessVersion", "0.145.0").unwrap());
        let started = service
            .lane_at(0)
            .sessions
            .start_session(old)
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let mut lease = service
            .lane_at(0)
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                artifact_digest: "44".repeat(32),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let binding = service.codex_registration().refresh().unwrap();
        let mut old_capabilities = binding.capability_ceiling().unwrap();
        old_capabilities.protocol_version =
            RequiredText::new("protocolVersion", "codex-app-server-0.145.0-7b0ab167").unwrap();
        old_capabilities.active_turn_steer = false;
        old_capabilities.child_transcript_read = false;
        lease
            .complete_activation(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                binding_digest: "f8c2b995be263fe4bf00974043312673378081734ca8c98155d2a33887444b9e"
                    .into(),
                artifact_digest: "44".repeat(32),
                external_session_id: ExternalId::new("externalSessionId", "old-thread").unwrap(),
                capabilities_json: serde_json::to_string(&old_capabilities).unwrap(),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        lease
            .park_after_zero_process(
                ParkSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    exit_kind: RequiredText::new("exitKind", "testIdle").unwrap(),
                    now_millis: crate::persistence::now_millis(),
                },
                ZeroProcessProof::confirmed(owner),
            )
            .await
            .unwrap();
        let old_text = "Readable history from the earlier Codex version";
        let body = serde_json::json!({
            "kind": "text",
            "payload": { "role": "user", "text": old_text }
        })
        .to_string();
        let digest = format!("{:x}", sha2::Sha256::digest(body.as_bytes()));
        database
            .connection()
            .execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at) VALUES('old-codex-turn',?1,?2,?3,1,0,'old-codex-intent','completed','correlated',0,1,1,1)",
                rusqlite::params![project_id.as_str(), chat_id.as_str(), started.session_id.as_str()],
            )
            .unwrap();
        database
            .connection()
            .execute(
                "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('old-codex-item',?1,?2,?3,'old-codex-turn',1,0,'primary',NULL,'text','completed',NULL,NULL,?4,?5,?6,0,1,1,1)",
                rusqlite::params![project_id.as_str(), chat_id.as_str(), started.session_id.as_str(), body, body.len() as i64, digest],
            )
            .unwrap();
        let page = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        assert!(
            page.sessions
                .as_slice()
                .iter()
                .any(|session| session.id == started.session_id)
        );
        let old_turn = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.id.as_str() == "old-codex-turn")
            .expect("old Codex Turn stays readable from SQLite");
        assert_eq!(old_turn.items.as_slice().len(), 1);
        assert!(matches!(
            &old_turn.items.as_slice()[0].content,
            crate::acp::projection::ItemContent::Text { text, .. } if text.as_str() == old_text
        ));
        let authority = codex_root(&project_id, &project.roots[0].id, &root);
        let admitted = service
            .binding_registry()
            .resolve(crate::acp::bindings::codex::CODEX_BINDING_ID)
            .unwrap();
        let result = service.prepare_session_activation(
            admitted,
            ActivateSessionCommand {
                project_id,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id,
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                artifact_digest: "55".repeat(32),
                now_millis: crate::persistence::now_millis(),
            },
            crate::acp::bindings::registry::BindingProjectRoot::Codex(authority),
        );
        assert!(matches!(
            result,
            Err(super::CodexSessionError::EarlierCodexVersion)
        ));
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn abort_activation_after_zero_restores_created_cold_and_is_idempotent() {
    tauri::async_runtime::block_on(async {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new(database.clone()).unwrap();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = service
            .lane_at(0)
            .sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: crate::acp::intent::BindingId::new("bindingId", "test-abort").unwrap(),
                binding_digest: "11".repeat(32),
                harness_version: None,
                model_id: None,
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let mut lease = service
            .lane_at(0)
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                artifact_digest: "22".repeat(32),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let abort = AbortActivationCommand {
            project_id,
            chat_id,
            session_id: started.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            binding_digest: "11".repeat(32),
            artifact_digest: "22".repeat(32),
            category: ActivationAbortCategory::AssemblyFailure,
            now_millis: crate::persistence::now_millis(),
        };
        assert!(
            database
                .connection()
                .execute(
                    "UPDATE agent_sessions SET state='created',residency='cold' WHERE id=?1",
                    [started.session_id.as_str()],
                )
                .is_err(),
            "rollback without the matching aborted epoch must fail"
        );
        let mutation = lease
            .abort_activation_after_zero_process(
                abort.clone(),
                ZeroProcessProof::confirmed(owner.clone()),
            )
            .await
            .unwrap();
        assert_eq!(mutation.state, crate::acp::lifecycle::SessionState::Created);
        let row: (String, String, String) = database
            .connection()
            .query_row(
                "SELECT s.state,s.residency,e.state FROM agent_sessions s
             JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.id=?1 AND e.runtime_epoch=1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, ("created".into(), "cold".into(), "aborted".into()));
        let duplicate =
            crate::acp::store::abort_activation(&mut database.connection(), abort.clone()).unwrap();
        assert_eq!(
            duplicate.disposition,
            crate::acp::store::MutationDisposition::Duplicate
        );
        let second = crate::acp::store::activate_session(
            &mut database.connection(),
            ActivateSessionCommand {
                project_id: abort.project_id.clone(),
                chat_id: abort.chat_id.clone(),
                expected_projection_revision: mutation.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(2).unwrap(),
                artifact_digest: "22".repeat(32),
                now_millis: crate::persistence::now_millis(),
            },
        )
        .unwrap();
        assert!(
            crate::acp::store::abort_activation(&mut database.connection(), abort).is_err(),
            "an older aborted epoch must not masquerade as an idempotent retry"
        );
        let current: (String, String, String) = database
            .connection()
            .query_row(
                "SELECT s.state,s.residency,e.state FROM agent_sessions s
                 JOIN session_runtime_epochs e ON e.session_id=s.id
                 WHERE s.id=?1 AND e.runtime_epoch=2",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            current,
            ("starting".into(), "hot".into(), "starting".into())
        );
        assert_eq!(second.state, crate::acp::lifecycle::SessionState::Starting);
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn activation_abort_category_is_a_closed_non_diagnostic_allowlist() {
    assert_eq!(
        ActivationAbortCategory::parse("launchFailure"),
        Some(ActivationAbortCategory::LaunchFailure)
    );
    assert_eq!(ActivationAbortCategory::parse("C:\\secret\\path"), None);
    assert_eq!(ActivationAbortCategory::parse("arbitrary diagnostic"), None);
}

#[test]
fn abort_resume_restores_ready_cold_and_retains_external_identity() {
    tauri::async_runtime::block_on(async {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new(database.clone()).unwrap();
        let started = service
            .lane_at(0)
            .sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: service
                    .load_chat_page(project_id.clone(), chat_id.clone(), None)
                    .unwrap()
                    .projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: crate::acp::intent::BindingId::new("bindingId", "test-resume-abort")
                    .unwrap(),
                binding_digest: "33".repeat(32),
                harness_version: None,
                model_id: None,
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let owner_one = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let mut first = service
            .lane_at(0)
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner_one.runtime_epoch,
                artifact_digest: "44".repeat(32),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let capabilities = serde_json::to_string(&capabilities()).unwrap();
        let ready = first
            .complete_activation(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner_one.runtime_epoch,
                binding_digest: "33".repeat(32),
                artifact_digest: "44".repeat(32),
                external_session_id: ExternalId::new("external", "durable-external").unwrap(),
                capabilities_json: capabilities.clone(),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        first
            .park_after_zero_process(
                ParkSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: owner_one.runtime_epoch,
                    exit_kind: RequiredText::new("exitKind", "testIdle").unwrap(),
                    now_millis: crate::persistence::now_millis(),
                },
                ZeroProcessProof::confirmed(owner_one),
            )
            .await
            .unwrap();
        let owner_two = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        };
        let mut second = service
            .lane_at(0)
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: ready.projection_revision.next().unwrap(),
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id.clone(),
                runtime_epoch: owner_two.runtime_epoch,
                artifact_digest: "44".repeat(32),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        second
            .abort_activation_after_zero_process(
                AbortActivationCommand {
                    project_id,
                    chat_id,
                    session_id: started.session_id.clone(),
                    runtime_epoch: owner_two.runtime_epoch,
                    binding_digest: "33".repeat(32),
                    artifact_digest: "44".repeat(32),
                    category: ActivationAbortCategory::RuntimeHandoffFailure,
                    now_millis: crate::persistence::now_millis(),
                },
                ZeroProcessProof::confirmed(owner_two),
            )
            .await
            .unwrap();

        let row: (String, String, String, String) = database
            .connection()
            .query_row(
                "SELECT state,residency,external_session_id,capabilities_json
                 FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "ready");
        assert_eq!(row.1, "cold");
        assert_eq!(row.2, "durable-external");
        assert_eq!(row.3, capabilities);
        service.shutdown_blocking().unwrap();
    });
}

#[cfg(windows)]
pub(super) fn seed_codex_discovery(
    service: &AcpService,
    project_root: &CodexProjectRootAuthority,
    model: Option<crate::acp::intent::ModelId>,
) {
    let binding = service.codex_registration().refresh().unwrap();
    service
        .codex_discovery_manager()
        .seed_session_validation_for_test(project_root, binding.binding_digest(), model);
}

#[cfg(windows)]
fn codex_root(
    project_id: &SageProjectId,
    root_id: &str,
    root: &std::path::Path,
) -> CodexProjectRootAuthority {
    CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            SageRootId::parse(root_id).unwrap(),
            root,
        )
        .unwrap(),
    )
}

#[cfg(windows)]
fn codex_session_setup() -> (
    Database,
    AcpService,
    crate::domain::ProjectSummary,
    SageProjectId,
    SageChatId,
    std::path::PathBuf,
) {
    let registration = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id.clone()).unwrap();
    let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
    let service = AcpService::new_with_codex_for_test(database.clone(), registration).unwrap();
    (database, service, project, project_id, chat_id, root)
}

#[cfg(windows)]
fn codex_start_command(
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    revision: ProjectionRevision,
) -> StartSessionCommand {
    StartSessionCommand {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        expected_projection_revision: revision,
        client_intent_id: ClientIntentId::new(),
        binding_id: crate::acp::intent::BindingId::new("bindingId", "codex-app-server").unwrap(),
        binding_digest: "forged".into(),
        harness_version: None,
        model_id: None,
        now_millis: crate::persistence::now_millis(),
    }
}

#[test]
fn renderer_recreation_snapshot_covers_the_exact_native_invariance_boundary() {
    #[cfg(windows)]
    let _serial = crate::test_wait::serial_process_guard();

    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    database
        .connection()
        .execute_batch(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
             VALUES('project-reload','Reload',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES('chat-reload','project-reload',0,'Reload',7,1,1);",
        )
        .unwrap();
    let service = AcpService::new(database.clone()).unwrap();
    let project_id = SageProjectId::parse("project-reload").unwrap();
    let chat_id = SageChatId::parse("chat-reload").unwrap();

    let before = service
        .renderer_recreation_native_snapshot(&project_id, &chat_id)
        .unwrap();
    let evidence_policy: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/evidence-policy-v1.json"
    ))
    .unwrap();
    let observation = &evidence_policy["rendererRecreationObservation"];
    let contracted_tables = observation["sqliteTables"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        before.table_names().as_slice(),
        contracted_tables.as_slice()
    );
    assert_eq!(
        before.maximum_rows_per_table(),
        observation["maximumRowsPerTable"].as_u64().unwrap()
    );
    assert_eq!(before.table_rows("chats"), Some(1));
    assert_eq!(before.chat_projection_revision(), 7);
    assert_eq!(before.runtime_slot_active(), 0);
    assert_eq!(before.live_process_tree_reservations(), 0);
    assert_eq!(
        before,
        service
            .renderer_recreation_native_snapshot(&project_id, &chat_id)
            .unwrap()
    );

    database
        .connection()
        .execute(
            "UPDATE chats SET title='Changed' WHERE id='chat-reload'",
            [],
        )
        .unwrap();
    let changed = service
        .renderer_recreation_native_snapshot(&project_id, &chat_id)
        .unwrap();
    assert_ne!(before, changed);
    assert_eq!(changed.chat_projection_revision(), 7);
}

struct StaticAuthority(IntentSnapshot);

impl ProjectIntentAuthority for StaticAuthority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

struct GatedSink {
    events: Mutex<Vec<ProjectionChangedV1>>,
    entered: tokio::sync::Notify,
    completed: tokio::sync::Notify,
    release: tokio::sync::Semaphore,
}

impl Default for GatedSink {
    fn default() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            entered: tokio::sync::Notify::new(),
            completed: tokio::sync::Notify::new(),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

impl NotificationSink for GatedSink {
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            self.entered.notify_one();
            self.release
                .acquire()
                .await
                .map_err(|_| NotificationError::SinkUnavailable)?
                .forget();
            self.completed.notify_one();
            Ok(())
        })
    }
}

struct Engine(Arc<Mutex<Vec<&'static str>>>);

impl RuntimeEngine for Engine {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        self.0.lock().unwrap().push("activate");
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.0.lock().unwrap().push("prepare");
        Ok(SageTurnId::parse("turn-service").unwrap())
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.0.lock().unwrap().push("dispatch");
        Ok(PromptWrite::Complete(turn_id.clone()))
    }
    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Ok(PromptWrite::Complete(turn_id.clone()))
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
        self.0.lock().unwrap().push("shutdown");
        Ok(())
    }
}

struct BlockingShutdownEngine(Arc<(Mutex<bool>, Condvar)>);

impl RuntimeEngine for BlockingShutdownEngine {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _deadline: std::time::Instant) -> Result<(), HostError> {
        let (released, ready) = &*self.0;
        let mut released = released.lock().unwrap();
        while !*released {
            released = ready.wait(released).unwrap();
        }
        Ok(())
    }
}

#[test]
fn service_keeps_background_turn_live_and_stops_runtime_before_journal_drop() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let runtime = Arc::new(RuntimeHost::start(Engine(Arc::clone(&events))).unwrap());
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-service").unwrap(),
        chat_id: SageChatId::parse("chat-service").unwrap(),
        session_id: SageAgentSessionId::parse("session-service").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    runtime.activate(owner.clone()).unwrap();
    runtime
        .send(HostPromptIntent {
            command: crate::acp::store::PrepareTurnCommand {
                project_id: owner.project_id.clone(),
                project_generation: ProjectGeneration::new(1).unwrap(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::parse("intent-service").unwrap(),
                prompt: Prompt::new("prompt", "hello").unwrap(),
                now_millis: crate::persistence::now_millis(),
            },
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    service.install_runtime_for_test(owner, Arc::clone(&runtime));

    service
        .project_switched(&SageProjectId::parse("project-other").unwrap())
        .unwrap();
    assert_eq!(runtime.state().unwrap(), HostState::Running);
    service.shutdown_blocking().unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        ["activate", "prepare", "dispatch", "shutdown"]
    );
}

#[test]
fn final_window_shutdown_returns_at_deadline_and_retries_the_same_actor() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let runtime = Arc::new(
        RuntimeHost::start_with_timeout(
            BlockingShutdownEngine(Arc::clone(&gate)),
            Duration::from_millis(20),
        )
        .unwrap(),
    );
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-timeout").unwrap(),
        chat_id: SageChatId::parse("chat-timeout").unwrap(),
        session_id: SageAgentSessionId::parse("session-timeout").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    runtime
        .without_timeout_override(|| runtime.activate(owner.clone()))
        .unwrap();
    service.install_runtime_for_test(owner, Arc::clone(&runtime));

    assert_eq!(service.shutdown_blocking(), Err(HostError::Deadline));
    assert!(service.journal.batch(JournalLane::Control).is_ok());

    let (released, ready) = &*gate;
    *released.lock().unwrap() = true;
    ready.notify_all();
    runtime
        .without_timeout_override(|| service.shutdown_blocking())
        .unwrap();
}

#[test]
fn desktop_notifier_starts_without_an_entered_tokio_runtime() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let service =
        AcpService::new_with_notification_sink(database, Arc::new(GatedSink::default())).unwrap();
    service.shutdown_blocking().unwrap();
}

#[test]
fn running_turn_terminal_after_project_switch_is_durable_but_cannot_touch_active_projection() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let base = std::env::temp_dir().join(format!("sage-acp-switch-{}", uuid::Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project_a = database.create_project_from_root(&root_a).unwrap();
        let project_b = database.create_project_from_root(&root_b).unwrap();
        let snapshot_b = database.bootstrap_snapshot().unwrap();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_a.id.clone(),
                current_layout: layout(&snapshot_b),
            })
            .unwrap();
        let project_a_id = SageProjectId::parse(project_a.id).unwrap();
        let project_b_id = SageProjectId::parse(project_b.id).unwrap();
        let chat_a = SageChatId::parse("chat-switch-a").unwrap();
        let chat_b = SageChatId::parse("chat-switch-b").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute_batch(&format!(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES ('{}','{}',1,'A',0,{now},{now});
                 INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES ('{}','{}',1,'B',0,{now},{now});",
                chat_a.as_str(), project_a_id.as_str(), chat_b.as_str(), project_b_id.as_str()
            ))
            .unwrap();

        let sink = Arc::new(GatedSink::default());
        let entered = sink.entered.notified();
        let service = AcpService::new_with_notification_sink(database.clone(), sink.clone()).unwrap();
        let initial_a = service
            .load_chat_page(project_a_id.clone(), chat_a.clone(), None)
            .unwrap();
        let binding = "11".repeat(32);
        let artifact = "22".repeat(32);
        let started = service
            .lane_at(0).sessions
            .start_session(StartSessionCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                expected_projection_revision: initial_a.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-switch-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-switch").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now + 1,
            })
            .await
            .unwrap();
        entered.await;
        let epoch = RuntimeEpoch::new(1).unwrap();
        let mut tree = service
            .lane_at(0).sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-switch-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-switch").unwrap();
        let ready = tree
            .complete_activation(CompleteActivationCommand {
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding.clone(),
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 3,
            })
            .await
            .unwrap();
        let snapshot = service
            .intent_snapshot(project_a_id.clone(), ProjectGeneration::ZERO, chat_a.clone())
            .unwrap();
        let sends = SendCoordinator::new(
            Arc::new(StaticAuthority(snapshot)),
            service.journal.clone(),
            service.notifier.as_ref().unwrap().clone(),
        );
        let prepared = tree
            .prepare_turn(
                &sends,
                PrepareTurnCommand {
                    project_id: project_a_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_a.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready.projection_revision,
                    client_intent_id: ClientIntentId::parse("intent-switch-turn").unwrap(),
                    prompt: Prompt::new("prompt", "continue in background").unwrap(),
                    now_millis: now + 4,
                },
            )
            .await
            .unwrap();
        let PrepareDispatch::Applied(mut dispatch) = prepared else {
            panic!("new Turn")
        };
        let sending = dispatch.begin_send(now + 5).await.unwrap();
        dispatch
            .observe(
                crate::acp::store::SendObservation::CompleteBytes,
                None,
                now + 6,
            )
            .await
            .unwrap();

        let snapshot_a = database.bootstrap_snapshot().unwrap();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_b_id.as_str().to_owned(),
                current_layout: layout(&snapshot_a),
            })
            .unwrap();
        service.project_switched(&project_b_id).unwrap();
        let before_b = service
            .load_chat_page(project_b_id.clone(), chat_b.clone(), None)
            .unwrap();
        sink.events.lock().unwrap().clear();
        let notifier = service.notifier.as_ref().unwrap();
        let wakes = notifier.scheduled_wakes();

        let active_turn = ActiveTurnIntent::store_minted(dispatch.turn_id().clone()).unwrap();
        let scope = TurnScope {
            project_id: project_a_id.clone(),
            chat_id: chat_a.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            turn_id: dispatch.turn_id().clone(),
            binding_digest: binding,
        };
        let mut adapter_journal = SqliteAdapterBatchJournal::new(
            service.journal.clone(),
            SessionPumpIntent {
                binding_digest: [0x11; 32],
                project_id: project_a_id.clone(),
                chat_id: chat_a.clone(),
                session_id: started.session_id.clone(),
                external_session_id: external_session,
                runtime_epoch: epoch,
            }
            .bind_turn(active_turn),
        );
        let receipt = adapter_journal
            .commit(
                vec![
                    DurableAdapterEvent::Turn {
                        scope: scope.clone(),
                        state: TurnState::Running,
                        now_millis: now + 7,
                    },
                    DurableAdapterEvent::Turn {
                        scope,
                        state: TurnState::Completed,
                        now_millis: now + 8,
                    },
                ],
                2,
            )
            .await
            .unwrap();
        assert_eq!(
            notifier
                .publish_committed(ProjectionChangedV1 {
                    project_id: project_a_id.clone(),
                    chat_id: chat_a.clone(),
                    session_id: started.session_id.clone(),
                    projection_revision: receipt.projection_revision,
                    child_projection_epoch: 0,
                })
                .unwrap(),
            PublishOutcome::SuppressedInactive
        );
        assert_eq!(notifier.scheduled_wakes(), wakes);
        dispatch.accept_adapter_commit(receipt.turn_cursor()).unwrap();

        let terminal: (String, Option<i64>) = database
            .connection()
            .query_row(
                "SELECT state,terminal_at FROM turns WHERE id=?1",
                [dispatch.turn_id().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(terminal, ("completed".into(), Some(now + 8)));
        let active = service.lane_at(0).scheduler.active().unwrap();
        assert_eq!(active.tree.project_id, project_a_id);
        assert!(!active.turn_reserved);
        assert_eq!(active.turn_id, None);
        let after_b = service
            .load_chat_page(project_b_id, chat_b, None)
            .unwrap();
        assert_eq!(after_b, before_b);
        assert!(sink.events.lock().unwrap().is_empty());
        assert_eq!(notifier.recovery(), None);

        let completed = sink.completed.notified();
        sink.release.add_permits(1);
        completed.await;
        service.shutdown_blocking().unwrap();
        drop(service);
        drop(database);
        fs::remove_dir_all(base).unwrap();
        assert_eq!(sending.state, TurnState::Sending);
    });
}

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    }
}
