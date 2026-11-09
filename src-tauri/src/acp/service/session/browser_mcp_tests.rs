use super::*;
use crate::acp::{
    bindings::{
        adapter::{AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper},
        codex::CodexBindingRegistration,
        registry::BindingProjectRoot,
    },
    contract::{ClientIntentId, RuntimeEpoch, SageChatId, SageProjectId},
    mcp::McpAdmission,
};

#[test]
fn non_admitted_browser_version_prepares_a_normal_session_and_releases_guard() {
    browser_activation("codex-cli 0.166.0", false);
}

#[test]
fn admitted_browser_version_keeps_mcp_and_guard() {
    browser_activation(
        &crate::acp::bindings::codex::test_identity::version_line("codex-cli"),
        true,
    );
}

fn browser_activation(version: &str, browser_admitted: bool) {
    tauri::async_runtime::block_on(async {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let fixture_root = root
            .join("work")
            .join(format!("codex-browser-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&fixture_root).unwrap();
        let fixture_root = fixture_root.canonicalize().unwrap();
        assert!(fixture_root.starts_with(&root));
        let executable = fixture_root.join("codex.exe");
        std::fs::write(&executable, b"@echo off\r\n").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();
        registration
            .authenticate_observed_for_test(executable.clone(), version.into())
            .unwrap();
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(database, registration).unwrap();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let project_root =
            super::super::tests::codex_root(&project_id, &project.roots[0].id, &root);
        if browser_admitted {
            super::super::tests::seed_codex_discovery(&service, &project_root, None);
        }
        let started = service
            .start_codex_session(
                &project_root,
                super::super::tests::codex_start_command(&project_id, &chat_id, revision),
            )
            .await
            .unwrap();
        let mut command = ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "forged".into(),
            now_millis: 1,
        };
        let (guard, revoked) = super::super::browser_mcp::BrowserMcpActivationGuard::for_test();
        let mcp = McpAdmission::browser_stdio(
            r"C:\Sage\sage.exe".into(),
            [
                ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
                ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
                ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
                ("SAGE_BROWSER_MCP_TOKEN".into(), "secret-token".into()),
            ],
        )
        .unwrap();
        let mut prepared = service
            .prepare_session_activation_inner(
                service
                    .binding_registry()
                    .resolve("codex-app-server")
                    .unwrap(),
                &mut command,
                BindingProjectRoot::Codex(project_root),
                |_| (mcp, Some(guard)),
            )
            .expect("optional browser availability must not block the session");
        assert_eq!(prepared.browser_mcp_guard.is_some(), browser_admitted);
        assert_eq!(
            revoked.load(std::sync::atomic::Ordering::SeqCst),
            !browser_admitted
        );
        let launch = prepared.runtime.launch_spec().unwrap();
        assert_eq!(
            launch
                .environment
                .as_slice()
                .iter()
                .any(|(name, value)| name == "SAGE_BROWSER_MCP_TOKEN" && value == "secret-token"),
            browser_admitted
        );
        if !browser_admitted {
            assert!(
                !launch
                    .environment
                    .as_slice()
                    .iter()
                    .any(|(name, _)| name.to_string_lossy().starts_with("SAGE_BROWSER_MCP_"))
            );
        }
        let ActivationMode::New { native_root, .. } = &prepared.intent.mode else {
            panic!("a fresh session must start a new native thread")
        };
        assert_runtime_browser_contract(
            &mut prepared.runtime,
            version,
            native_root,
            browser_admitted,
        );
        drop(prepared);
        assert!(revoked.load(std::sync::atomic::Ordering::SeqCst));
        service.shutdown_blocking().unwrap();
        assert!(fixture_root.starts_with(&root));
        std::fs::remove_dir_all(fixture_root).unwrap();
    });
}

struct NoResources;
impl ResourceMapper for NoResources {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        panic!("this protocol exchange has no file resources")
    }
}

fn frame(value: serde_json::Value) -> crate::acp::transport::Frame {
    let bytes = format!("{value}\n");
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(bytes.as_bytes())
        .unwrap()
        .unwrap()
}

fn assert_runtime_browser_contract(
    runtime: &mut NativeRuntime,
    version: &str,
    native_root: &str,
    browser_admitted: bool,
) {
    use serde_json::{Value, json};
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    let request: Value = serde_json::from_slice(initialize.frame().payload()).unwrap();
    runtime
        .resolve_egress(initialize.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = runtime.ingest(&frame(json!({"id":request["id"],"result":{
        "userAgent":format!("sage/{} (Windows 10.0.26200; x86_64) dumb (sage; 0.1.0)", version.split_whitespace().last().unwrap()),
        "codexHome":r"C:\fixture-home", "platformFamily":"windows", "platformOs":"windows"
    }})), &NoResources).unwrap();
    runtime
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: native_root.to_owned(),
            model: None,
        })
        .unwrap();
    let request: Value = serde_json::from_slice(start.frame().payload()).unwrap();
    assert_eq!(request["params"]["config"].is_object(), browser_admitted);
    if browser_admitted {
        assert_eq!(
            request["params"]["config"]["mcp_servers"]["sage-browser"]["command"],
            r"C:\Sage\sage.exe"
        );
    }
    assert!(!request.to_string().contains("secret-token"));
    let ready = runtime
        .ingest(
            &frame(json!({"id":request["id"],"result":{"thread":{"id":"thread-browser"}}})),
            &NoResources,
        )
        .unwrap();
    assert!(matches!(
        ready.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
    runtime
        .command(AdapterCommand::Prompt {
            prompt: crate::acp::intent::Prompt::new("prompt", "Say hello").unwrap(),
            client_message_id: crate::acp::contract::RequiredText::new("client", "browser-degrade")
                .unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let running = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-browser", "turn":{"id":"turn-browser", "status":"inProgress"}
            }})),
            &NoResources,
        )
        .unwrap();
    if browser_admitted {
        assert!(matches!(running.events(), [AdapterEvent::Turn { .. }]));
    } else {
        assert!(
            matches!(running.events(), [AdapterEvent::Turn { .. }, AdapterEvent::Error(crate::acp::intent::NormalizedPayload::Error { code, message, retryable:false, .. })]
            if code.as_str() == "browserMcpUnavailable" && message.as_str().contains("unavailable for this Codex version") && message.as_str().contains("continue chatting"))
        );
    }
    let duplicate = runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-browser", "turn":{"id":"turn-browser", "status":"inProgress"}
            }})),
            &NoResources,
        )
        .unwrap();
    assert!(
        !duplicate
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::Error(_)))
    );
}
