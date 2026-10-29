use super::*;
use crate::acp::{
    bindings::{adapter::HarnessAdapter, registry::BindingRegistry},
    contract::{RuntimeEpoch, SageAgentSessionId},
};
use std::fs;

#[test]
fn fixture_registry_isolated_and_native_activation_revalidates_without_consuming_catalog_reads() {
    let directory = std::env::temp_dir().join(format!("sage-nc1b-{}", uuid::Uuid::new_v4()));
    let profile = directory.join("profile");
    let project = directory.join("project");
    fs::create_dir_all(profile.join("durable")).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(
        profile.join(".sage-disposable-profile-v1"),
        b"sage-disposable-profile-v1\n",
    )
    .unwrap();
    let artifact = directory.join("fixture.exe");
    fs::write(&artifact, b"synthetic fixture").unwrap();
    use sha2::{Digest, Sha256};
    let selection = crate::launch_profile::FixtureSelection {
        proof_root: directory.canonicalize().unwrap(),
        profile_root: profile.canonicalize().unwrap(),
        durable_dir: profile.join("durable").canonicalize().unwrap(),
        project_root: project.canonicalize().unwrap(),
        artifact_path: artifact.canonicalize().unwrap(),
        artifact_sha256: Sha256::digest(b"synthetic fixture").into(),
        artifact_bytes: 17,
        case: "pi-turn-completed".into(),
    };
    // The production registry routes pi-rpc to the curated native binding,
    // which stays unadmitted until discovery finds an installed Pi.
    assert!(BindingRegistry::default().resolve("pi-rpc").is_err());
    let registry = BindingRegistry::desktop_pi_fixture(selection.clone()).unwrap();
    let catalog = crate::harnesses::HarnessCatalog::with_registry(std::sync::Arc::new(registry));
    catalog.apply_presence(Vec::new());
    let definitions = catalog.definitions();
    let pi = definitions
        .iter()
        .find(|definition| definition.id == "pi")
        .unwrap();
    assert_eq!(pi.install_state, "available");
    assert_eq!(pi.binding_id.as_deref(), Some("pi-rpc"));
    pi.validate_transport_contract().unwrap();
    let registry = catalog.binding_registry();
    for binding in ["codex-app-server", "claude-agent-acp", "opencode-acp"] {
        assert!(
            !registry
                .route(binding)
                .unwrap()
                .session_operations_enabled()
        );
        assert!(registry.resolve(binding).is_err());
    }
    let admitted = registry.resolve("pi-rpc").unwrap();
    assert!(!admitted.has_model_discovery().unwrap());
    assert!(!admitted.supported_capabilities().unwrap().close_session);
    let binding = fixture_admission::PiFixtureBinding::admit(selection.clone()).unwrap();
    assert!(
        !selection
            .durable_dir
            .join("codex-fixture-launch-once-v1")
            .exists()
    );
    let root = |path: &std::path::Path| {
        crate::project_registry::ProjectRootAuthority::for_test(
            crate::acp::contract::SageProjectId::parse("fixture-project").unwrap(),
            crate::acp::contract::ProjectGeneration::ZERO,
            crate::acp::contract::SageRootId::parse("fixture-root").unwrap(),
            path,
        )
        .unwrap()
    };
    let mcp = crate::acp::mcp::McpAdmission::browser_stdio(
        selection.artifact_path.to_string_lossy().into_owned(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "fixture-session".into()),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "synthetic".into()),
        ],
    )
    .unwrap();
    assert!(binding.launch(1, root(&project), &mcp).is_err());
    let foreign = directory.join("foreign");
    fs::create_dir_all(&foreign).unwrap();
    assert!(
        binding
            .launch(1, root(&foreign), &crate::acp::mcp::McpAdmission::default())
            .is_err()
    );
    assert!(
        !selection
            .durable_dir
            .join("codex-fixture-launch-once-v1")
            .exists()
    );
    let mut native = admitted
        .into_factory(
            crate::acp::bindings::registry::BindingProjectRoot::Pi(root(&project)),
            crate::acp::mcp::McpAdmission::default(),
        )
        .construct(
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("fixture-native-session").unwrap(),
        )
        .unwrap();
    let spec = native.launch_spec().unwrap();
    assert_eq!(
        spec.artifact.canonical_absolute_path,
        selection.artifact_path
    );
    assert!(
        native.capabilities().is_none(),
        "activation is not ready before initialization"
    );
    assert_native_activation(native, &binding, &selection);
    assert!(
        binding
            .launch(1, root(&project), &crate::acp::mcp::McpAdmission::default())
            .is_err()
    );
    fs::write(artifact, b"changed artifact!").unwrap();
    assert!(binding.revalidate().is_err());
    fs::remove_dir_all(directory).unwrap();
}

fn assert_native_activation(
    mut runtime: crate::acp::bindings::runtime::NativeRuntime,
    binding: &fixture_admission::PiFixtureBinding,
    selection: &crate::launch_profile::FixtureSelection,
) {
    use crate::acp::{
        contract::{ProjectionRevision, SageChatId, SageProjectId},
        runtime::{
            ActivationFlush, ActivationIntent, ActivationJournal, ActivationJournalError,
            ActivationMode, ActivationPump, ActivationReceipt, ActivationTransport,
        },
        store::CompleteActivationCommand,
        transport::{FrameWriteOutcome, OutboundFrame},
    };

    struct Journal(Option<CompleteActivationCommand>);
    impl ActivationJournal for Journal {
        fn commit_ready<'a>(
            &'a mut self,
            command: CompleteActivationCommand,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ActivationReceipt, ActivationJournalError>>
                    + Send
                    + 'a,
            >,
        > {
            assert!(self.0.is_none());
            self.0 = Some(command);
            Box::pin(async {
                Ok(ActivationReceipt {
                    projection_revision: ProjectionRevision::new(1).unwrap(),
                })
            })
        }
    }

    struct Transport(Option<serde_json::Value>);
    impl ActivationTransport for Transport {
        fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
            assert!(self.0.is_none());
            self.0 = Some(serde_json::from_slice(frame.payload()).unwrap());
            FrameWriteOutcome::Complete
        }
    }

    struct Resources;
    impl crate::acp::bindings::adapter::ResourceMapper for Resources {
        fn map_existing(
            &self,
            _: &str,
        ) -> Result<
            crate::acp::intent::SafeResourceRef,
            crate::acp::intent::UnavailableResourceReason,
        > {
            Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
        }
    }

    let owner = crate::acp::scheduler::RuntimeOwner {
        project_id: SageProjectId::parse("fixture-project").unwrap(),
        chat_id: SageChatId::parse("fixture-chat").unwrap(),
        session_id: SageAgentSessionId::parse("fixture-native-session").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let channel = std::sync::Arc::new(crate::acp::session_config::ConfigChannel::default());
    channel.reserve(owner.clone());
    runtime.attach_config(owner.clone(), channel.clone());
    let registry = BindingRegistry::desktop_pi_fixture(selection.clone()).unwrap();
    let ceiling = registry
        .resolve("pi-rpc")
        .unwrap()
        .supported_capabilities()
        .unwrap();
    let mut pump = ActivationPump::new(
        runtime,
        Journal(None),
        Resources,
        ActivationIntent {
            binding_digest: binding.binding_digest(),
            artifact_digest: binding.artifact_digest().into(),
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            mode: ActivationMode::New {
                native_root: selection.project_root.to_str().unwrap().into(),
                model: None,
            },
            curated_ceiling: ceiling.clone(),
            sage_policy: ceiling,
        },
    )
    .unwrap();
    let mut transport = Transport(None);
    let model =
        serde_json::json!({"provider":"openai","id":"fixture-model","name":"Synthetic replay"});
    let alternative = serde_json::json!({"provider":"openai","id":"fixture-model-two","name":"Synthetic alternative"});
    tauri::async_runtime::block_on(async {
        pump.begin().unwrap();
        for (index, (command, data)) in [
            (
                "get_state",
                serde_json::json!({"model":model,"thinkingLevel":"medium"}),
            ),
            (
                "get_available_models",
                serde_json::json!({"models":[model, alternative]}),
            ),
            ("new_session", serde_json::json!({"cancelled":false})),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(!pump.completion_ready());
            assert_eq!(
                pump.flush_one(&mut transport).unwrap(),
                ActivationFlush::Written
            );
            let request = transport.0.take().unwrap();
            assert_eq!(request["type"], command);
            let mut response = serde_json::to_vec(&serde_json::json!({
                "type":"response","id":request["id"],"command":command,"success":true,"data":data
            }))
            .unwrap();
            response.push(b'\n');
            pump.push_stdout_chunk(&response, index as i64)
                .await
                .unwrap();
        }
    });
    assert!(pump.completion_ready());
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        ActivationFlush::Idle
    );
    let parts = pump.try_into_idle_parts().ok().unwrap();
    assert!(parts.journal().0.is_some());
    assert_eq!(
        parts.session().external_session_id.as_str(),
        "pi-session-1-1"
    );
    assert_eq!(parts.session().models.len(), 2);
    assert_eq!(parts.session().capabilities.model_count, 2);
    assert_eq!(parts.session().capabilities.config_count, 1);
    channel.publish(&owner);
    assert!(channel.live_snapshot(&owner).is_some());
}
