use super::*;
use crate::acp::bindings::registry::{BindingProjectRoot, BindingRegistry};
use crate::acp::lifecycle::Residency;
use crate::acp::runtime::HostPromptIntent;
use crate::acp::session_config::{ConfigId, ConfigIntent, ConfigKind, ConfigMutation, ConfigValue};
use crate::launch_profile::FixtureSelection;
use std::fs;
use std::sync::Arc;

struct Profile {
    directory: PathBuf,
    selection: FixtureSelection,
    database_path: PathBuf,
}

fn profile(case: &str) -> Profile {
    let directory =
        std::env::temp_dir().join(format!("sage-config-restart-{}", uuid::Uuid::new_v4()));
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
    let selection = FixtureSelection {
        proof_root: directory.canonicalize().unwrap(),
        profile_root: profile.canonicalize().unwrap(),
        durable_dir: profile.join("durable").canonicalize().unwrap(),
        project_root: project.canonicalize().unwrap(),
        artifact_bytes: fs::metadata(&artifact).unwrap().len(),
        artifact_path: artifact.canonicalize().unwrap(),
        artifact_sha256: sha256,
        case: case.into(),
    };
    let database_path = selection.durable_dir.join("sage.db");
    Profile {
        directory,
        selection,
        database_path,
    }
}

fn choose(setup: &FixtureSetup, owner: &RuntimeOwner, id: &str, value: &str) {
    let revision = setup
        .service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)
        .unwrap()
        .config_revision;
    setup
        .service
        .configure_session(ConfigIntent {
            owner: owner.clone(),
            expected_config_revision: revision,
            mutation: ConfigMutation::Option {
                config_id: ConfigId::new("configId", id).unwrap(),
                value: ConfigValue::Select {
                    value: ConfigId::new("value", value).unwrap(),
                },
            },
        })
        .unwrap_or_else(|error| panic!("choosing {id}={value} failed: {error:?}"));
}

fn shown(setup: &FixtureSetup, owner: &RuntimeOwner, id: &str) -> Option<String> {
    let snapshot = setup
        .service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)?;
    snapshot
        .config
        .config_options
        .as_slice()
        .iter()
        .find(|option| option.id.as_str() == id)
        .and_then(|option| match &option.kind {
            ConfigKind::Select { current_value, .. } => Some(current_value.as_str().to_owned()),
            _ => None,
        })
}

fn confirmed_model(setup: &FixtureSetup, owner: &RuntimeOwner) -> Option<String> {
    let snapshot = setup
        .service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)?;
    Some(snapshot.config.confirmed_model?.id.as_str().to_owned())
}

fn restored(setup: &FixtureSetup, owner: &RuntimeOwner) -> serde_json::Value {
    let page = setup.page(owner);
    assert!(page.session_config.is_none());
    serde_json::to_value(&page).unwrap()["restoredSessionConfig"].clone()
}

fn option_value(config: &serde_json::Value, id: &str) -> String {
    config["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == id)
        .map(|option| option["currentValue"].as_str().unwrap().to_owned())
        .unwrap_or_default()
}

fn wait_shown(setup: &FixtureSetup, owner: &RuntimeOwner, id: &str, value: &str) {
    setup.wait_until(|| shown(setup, owner, id).as_deref() == Some(value));
}

fn exit_gracefully(
    setup: FixtureSetup,
    owner: &RuntimeOwner,
) -> (SageProjectId, SageChatId, PathBuf, SageRootId) {
    setup.service.shutdown_blocking().unwrap();
    setup.wait_for_job_zero();
    let page = setup.page(owner);
    let session = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| session.id == owner.session_id)
        .unwrap();
    assert_eq!(session.state, SessionState::Ready);
    assert_eq!(session.residency, Residency::Cold);
    (
        setup.project_id.clone(),
        setup.chat_id.clone(),
        setup.root_path.clone(),
        setup.root_id.clone(),
    )
}

fn relaunch(
    profile: &Profile,
    registry: Arc<BindingRegistry>,
    (project_id, chat_id, root_path, root_id): (SageProjectId, SageChatId, PathBuf, SageRootId),
) -> FixtureSetup {
    let service = AcpService::new_with_registry_for_test(
        crate::persistence::Database::open(&profile.database_path).unwrap(),
        registry,
    )
    .unwrap();
    FixtureSetup::reopened(
        service,
        profile.database_path.clone(),
        project_id,
        chat_id,
        root_path,
        root_id,
    )
}

fn approve_permission(setup: &FixtureSetup, owner: &RuntimeOwner) {
    let open = || {
        setup
            .service
            .database
            .connection()
            .query_row(
                "SELECT id,turn_id FROM server_requests WHERE kind='permission' AND state IN ('received','asked')",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .ok()
    };
    setup.wait_until(|| open().is_some());
    let (request, turn) = open().unwrap();
    setup
        .service
        .resolve_permission_decision(crate::acp::service::PermissionDecisionIntent {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            turn_id: SageTurnId::parse(turn).unwrap(),
            request_id: crate::acp::contract::SageServerRequestId::parse(request).unwrap(),
            decision: crate::acp_protocol::permissions::PermissionDecision::AllowOnce,
        })
        .unwrap();
}

fn claude_root(setup: &FixtureSetup) -> BindingProjectRoot {
    BindingProjectRoot::Claude(
        ProjectRootAuthority::for_test(
            setup.project_id.clone(),
            ProjectGeneration::ZERO,
            setup.root_id.clone(),
            &setup.root_path,
        )
        .unwrap(),
    )
}

async fn activate_claude(setup: &FixtureSetup, owner: &RuntimeOwner) {
    setup
        .service
        .activate_session(
            setup
                .service
                .binding_registry()
                .resolve("claude-agent-acp")
                .unwrap(),
            setup.activate_command(owner),
            claude_root(setup),
        )
        .await
        .unwrap_or_else(|error| panic!("Claude activation failed: {error:?}"));
    setup.wait_for_ready(owner);
}

fn claude_turn_with_approval(setup: &FixtureSetup, owner: &RuntimeOwner) {
    let before = setup.completed_turns(owner);
    setup
        .service
        .send_session_turn(HostPromptIntent {
            command: setup.turn_command(owner),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::Ask,
        })
        .unwrap();
    approve_permission(setup, owner);
    setup.wait_for_completed_after(owner, &before);
}

#[test]
fn claude_config_choices_survive_a_graceful_restart() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let profile = profile("claude-config-resume");
        let registry = || {
            Arc::new(BindingRegistry::desktop_claude_fixture(profile.selection.clone()).unwrap())
        };
        let setup = FixtureSetup::open_with_registry_at_root(
            &profile.database_path,
            Some(profile.database_path.clone()),
            registry(),
            &profile.selection.project_root,
        )
        .await;
        let revision = setup
            .service
            .load_chat_page(setup.project_id.clone(), setup.chat_id.clone(), None)
            .unwrap()
            .projection_revision;
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
                &claude_root(&setup),
                start,
            )
            .await
            .unwrap();
        let first = RuntimeOwner {
            project_id: setup.project_id.clone(),
            chat_id: setup.chat_id.clone(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        activate_claude(&setup, &first).await;
        claude_turn_with_approval(&setup, &first);
        wait_shown(&setup, &first, "model", "fixture-model");
        choose(&setup, &first, "model", "fixture-model-deep");
        wait_shown(&setup, &first, "model", "fixture-model-deep");
        choose(&setup, &first, "effort", "high");
        wait_shown(&setup, &first, "effort", "high");

        let place = exit_gracefully(setup, &first);
        let setup = relaunch(&profile, registry(), place);
        let resumed = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..first.clone()
        };
        let restored = restored(&setup, &resumed);
        assert_eq!(option_value(&restored, "model"), "fixture-model-deep");
        assert_eq!(option_value(&restored, "effort"), "high");
        activate_claude(&setup, &resumed).await;
        wait_shown(&setup, &resumed, "model", "fixture-model-deep");
        wait_shown(&setup, &resumed, "effort", "high");
        claude_turn_with_approval(&setup, &resumed);
        assert_eq!(
            shown(&setup, &resumed, "model").as_deref(),
            Some("fixture-model-deep")
        );
        assert_eq!(shown(&setup, &resumed, "effort").as_deref(), Some("high"));
        setup.service.shutdown_blocking().unwrap();
        setup.wait_for_job_zero();
        drop(setup);
        fs::remove_dir_all(&profile.directory).unwrap();
    });
}

#[test]
fn codex_model_and_effort_survive_a_graceful_restart() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let profile = profile("codex-config-resume");
        let registry =
            || Arc::new(BindingRegistry::desktop_codex_fixture(profile.selection.clone()).unwrap());
        let setup = FixtureSetup::open_with_registry_at_root(
            &profile.database_path,
            Some(profile.database_path.clone()),
            registry(),
            &profile.selection.project_root,
        )
        .await;
        let root = setup.root();
        let binding = setup.service.codex_registration().refresh().unwrap();
        let request = crate::acp::ipc::GetCodexDiscoveryRequest {
            project_id: root.project_id().clone(),
            project_generation: root.project_generation(),
            binding_id: crate::acp::intent::BindingId::new("bindingId", binding.binding_id())
                .unwrap(),
        };
        let discovery = setup.service.codex_discovery_manager();
        let prepared = discovery
            .prepare(
                request,
                root,
                crate::acp::bindings::codex::CodexDiscoveryDeadline::new().unwrap(),
            )
            .unwrap();
        let menu = discovery.discover(prepared).unwrap();
        let listed: Vec<_> = menu.models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(listed, ["fixture-model", "fixture-model-deep"]);
        let deep = crate::acp::intent::ModelId::new("modelId", "fixture-model-deep").unwrap();
        let revision = setup
            .service
            .load_chat_page(setup.project_id.clone(), setup.chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let mut start = setup.start_command_for(&setup.chat_id, revision);
        start.model_id = Some(deep);
        let started = setup
            .service
            .start_codex_session(&setup.root(), start)
            .await
            .unwrap();
        let first = RuntimeOwner {
            project_id: setup.project_id.clone(),
            chat_id: setup.chat_id.clone(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        setup.activate(&first).await.unwrap();
        setup.wait_for_ready(&first);
        wait_shown(&setup, &first, "effort", "medium");
        assert_eq!(
            confirmed_model(&setup, &first).as_deref(),
            Some("fixture-model-deep")
        );
        setup.send_and_wait_completed(&first).await;
        choose(&setup, &first, "model", "fixture-model");
        wait_shown(&setup, &first, "model", "fixture-model");
        choose(&setup, &first, "effort", "high");
        wait_shown(&setup, &first, "effort", "high");

        let place = exit_gracefully(setup, &first);
        let setup = relaunch(&profile, registry(), place);
        let resumed = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..first.clone()
        };
        let restored = restored(&setup, &resumed);
        assert_eq!(option_value(&restored, "model"), "fixture-model");
        assert_eq!(option_value(&restored, "effort"), "high");
        setup
            .activate(&resumed)
            .await
            .unwrap_or_else(|error| panic!("Codex resume failed: {error:?}"));
        setup.wait_for_ready(&resumed);
        wait_shown(&setup, &resumed, "model", "fixture-model");
        wait_shown(&setup, &resumed, "effort", "high");
        setup.send_and_wait_completed(&resumed).await;
        assert_eq!(shown(&setup, &resumed, "effort").as_deref(), Some("high"));
        setup.service.shutdown_blocking().unwrap();
        setup.wait_for_job_zero();
        drop(setup);
        fs::remove_dir_all(&profile.directory).unwrap();
    });
}
