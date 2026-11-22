//! The compatibility contract is captured from the binding's real serializers.
//! Regenerate with SAGE_UPDATE_CODEX_REQUEST_EVIDENCE=1 and this test filter.
use super::super::request::{NativeProjectRoot, OutgoingRequest};
use super::*;
use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    background_terminals::TerminalAction,
    mcp::{McpAdmission, McpEnv, McpServerDescriptor},
};
use sage_acp_protocol::{access_mode::SessionAccessMode, intent::ImageMediaType};
use std::collections::BTreeMap;
use std::sync::Arc;

#[test]
fn outgoing_request_evidence_matches_real_serializers() {
    let root = NativeProjectRoot::new(Platform::LinuxX86_64, "/repo".into()).unwrap();
    let thread = RequiredText::new("thread", "thread-1").unwrap();
    let message = RequiredText::new("message", "message-1").unwrap();
    let model = RequiredText::new("model", "fixture-model").unwrap();
    let cursor = RequiredText::new("cursor", "next-page").unwrap();
    let effort = RequiredText::new("effort", "high").unwrap();
    let mut prompt = Prompt::new("prompt", "/pdf inspect").unwrap();
    prompt.images = PromptImages(Arc::new(vec![PromptImage {
        media_type: ImageMediaType::Png,
        bytes: vec![7; 3],
        preview: ImagePreview::Full,
    }]));
    let mut requests: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut capture = |bytes: &[u8]| {
        let mut value: Value = serde_json::from_slice(bytes).unwrap();
        // Correlation IDs vary independently of the payload shape.
        if value.get("id").is_some() {
            value["id"] = json!(1);
        }
        let prefix = if value.get("id").is_some() {
            "client"
        } else {
            "clientNotification"
        };
        let key = format!("{prefix}:{}", value["method"].as_str().unwrap());
        let variants = requests.entry(key).or_default();
        if !variants.contains(&value) {
            variants.push(value);
        }
    };
    let mut send = |request: Result<OutgoingRequest, request::RequestError>| {
        capture(&request.unwrap().bytes);
    };
    send(OutgoingRequest::initialize(1));
    send(OutgoingRequest::read(1, &thread));
    send(OutgoingRequest::fork(1, &thread));
    send(OutgoingRequest::fork_release(1, thread.as_str(), false));
    send(OutgoingRequest::fork_release(1, thread.as_str(), true));
    send(OutgoingRequest::unsubscribe(1, &thread));
    send(OutgoingRequest::interrupt(1, &thread, &message));
    send(OutgoingRequest::steer(
        1, &thread, &message, &prompt, &message,
    ));
    send(OutgoingRequest::rate_limits_read(1));
    send(OutgoingRequest::goal_get(1, &thread));
    send(OutgoingRequest::goal_pause(1, &thread));
    for action in [
        crate::acp::goal::GoalAction::Set {
            objective: "Ship safely".into(),
            token_budget: None,
        },
        crate::acp::goal::GoalAction::Set {
            objective: "Ship safely".into(),
            token_budget: Some(40000),
        },
        crate::acp::goal::GoalAction::ResumeExisting,
        crate::acp::goal::GoalAction::SetBudget {
            token_budget: 40000,
        },
    ] {
        send(OutgoingRequest::goal_set(1, &thread, &action));
    }
    send(OutgoingRequest::goal_clear(1, &thread));
    send(OutgoingRequest::thread_revert(1, &thread, "turn-1"));
    send(OutgoingRequest::mcp_list(1, &thread));
    send(OutgoingRequest::mcp_reload(1));
    send(OutgoingRequest::mcp_sign_in(1, &thread, "files"));
    for action in [
        TerminalAction::List,
        TerminalAction::Stop("process-1".into()),
        TerminalAction::StopAll,
    ] {
        send(OutgoingRequest::background_terminals(1, &thread, &action));
    }
    for target in [
        json!({"kind":"uncommittedChanges"}),
        json!({"kind":"baseBranch","branch":"main"}),
        json!({"kind":"commit","sha":"1a2b3c4"}),
        json!({"kind":"custom","instructions":"inspect errors"}),
    ] {
        let action = serde_json::from_value(json!({"kind":"review","target":target})).unwrap();
        send(OutgoingRequest::session_action(1, &thread, &action));
    }
    send(OutgoingRequest::session_action(
        1,
        &thread,
        &crate::acp::session_action::SessionAction::Compact {},
    ));
    let browser = McpAdmission::browser_stdio(
        "/bin/sage".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "fixture-token".into()),
        ],
    )
    .unwrap();
    let stdio = McpAdmission::for_test(vec![McpServerDescriptor::stdio_for_test(
        "files",
        "/bin/mcp",
        &["--root", "/repo"],
        vec![McpEnv::for_test("PUBLIC_MODE", "fixture")],
    )])
    .unwrap();
    for model in [None, Some(&model)] {
        for config in [
            None,
            browser.codex_config().unwrap(),
            stdio.codex_config().unwrap(),
        ] {
            for request in [
                OutgoingRequest::start(1, &root, model),
                OutgoingRequest::resume(1, &thread, &root, model),
            ] {
                let request = request.unwrap();
                send(match &config {
                    Some(config) => request.with_config(config.clone()),
                    None => Ok(request),
                });
            }
        }
    }
    for cursor in [None, Some(&cursor)] {
        send(OutgoingRequest::models(1, cursor));
        send(OutgoingRequest::session_list(
            1,
            &root,
            cursor.map(RequiredText::as_str),
            &["excluded".into()],
        ));
        for newest in [false, true] {
            send(OutgoingRequest::turns_list(
                1,
                &thread,
                cursor.map(RequiredText::as_str),
                newest,
            ));
        }
    }
    for reload in [false, true] {
        send(OutgoingRequest::skills_list(1, "/repo", reload));
    }
    for access_mode in [
        SessionAccessMode::ReadOnly,
        SessionAccessMode::WorkspaceWrite,
        SessionAccessMode::Ask,
    ] {
        for settings in [
            turn_policy::TurnSettings::default(),
            turn_policy::TurnSettings {
                model: Some(&model),
                effort: Some(&effort),
            },
        ] {
            let request = OutgoingRequest::turn_start_with_access_mode(
                1,
                &thread,
                &prompt,
                &message,
                access_mode,
                turn_policy::TurnPolicySurface::Supported,
                settings,
            )
            .unwrap();
            send(Ok(request.clone()));
            send(request.with_skill("pdf", "/skills/pdf/SKILL.md"));
        }
    }
    capture(&request::initialized_notification().unwrap());
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let mut auth =
        super::super::auth::CodexAuthProtocol::new(&packet, [9; 32], RuntimeEpoch::new(1).unwrap());
    for request in [
        auth.account_request(),
        auth.login_request(),
        auth.logout_request(),
        auth.api_key_login_request(
            &super::super::auth::NativeApiKey::try_from("fixture-key".to_owned()).unwrap(),
        ),
    ] {
        capture(request.unwrap().payload());
    }
    // NativeLoginId's parser is private to auth; obtain it through its real reply parser.
    let login = auth.login_request().unwrap();
    let mut decoder = crate::acp::transport::IncrementalFrameDecoder::new();
    let bytes = format!(
        "{}\n",
        json!({"id":login.id(),"result":{
            "type":"chatgpt","loginId":"7be3f527-7a5a-41e5-9e5f-a1a74b42f86b",
            "authUrl":"https://auth.openai.com/oauth/authorize?state=fixture"
        }})
    );
    let frame = decoder.push_chunk(bytes.as_bytes()).unwrap().unwrap();
    let (login_id, _) = auth.accept_login_started(&frame, login.id()).unwrap();
    capture(auth.cancel_request(&login_id).unwrap().payload());
    // Discovery has its own page-size serializer; capture both cursor forms
    // through the real handshake and pagination state machine as well.
    let mut discovery =
        CodexDiscoveryProtocol::new(&packet, [9; 32], RuntimeEpoch::new(1).unwrap());
    let initialize = discovery.initialize_request().unwrap();
    capture(initialize.payload());
    capture(
        discovery
            .accept_initialize(&reply(json!({"id":initialize.id(),"result":{
                "userAgent":user_agent("sage", "terminal"), "codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows", "platformOs":"windows"
            }})))
            .unwrap()
            .payload(),
    );
    let account = discovery.account_request().unwrap();
    capture(account.payload());
    discovery
        .accept_account(&reply(json!({"id":account.id(),"result":{
            "account":null,"requiresOpenaiAuth":false,"workspaceRouting":null
        }})))
        .unwrap();
    let first = discovery.model_request().unwrap();
    capture(first.payload());
    discovery
        .accept_model_page(&reply(json!({"id":first.id(),"result":{
            "data":[],"nextCursor":"next-page"
        }})))
        .unwrap();
    capture(discovery.model_request().unwrap().payload());
    // A new method must have a real serialized example before it can enter
    // the compatibility proof. Use the same dispatch source as the generator.
    for method in include_str!("../request.rs")
        .split("=> \"")
        .skip(1)
        .filter_map(|part| part.split('"').next())
    {
        assert!(
            requests.contains_key(&format!("client:{method}")),
            "uncaptured method {method}"
        );
    }
    for source in [
        include_str!("../auth.rs"),
        include_str!("../auth/api_key.rs"),
    ] {
        for method in source
            .split("self.request(")
            .skip(1)
            .filter_map(|part| part.trim_start().strip_prefix('"'))
            .filter_map(|part| part.split('"').next())
        {
            assert!(
                requests.contains_key(&format!("client:{method}")),
                "uncaptured auth method {method}"
            );
        }
    }
    for method in include_str!("../discovery/protocol.rs")
        .split("json_request(")
        .skip(1)
        .filter_map(|part| part.split_once(','))
        .filter_map(|(_, part)| part.trim_start().strip_prefix('"'))
        .filter_map(|part| part.split('"').next())
    {
        assert!(
            requests.contains_key(&format!("client:{method}")),
            "uncaptured discovery method {method}"
        );
    }
    // JSON serialization erases the distinction between a struct and a map.
    // Check each map annotation against distinct keys from the real builders.
    let open_objects = ["/params/config/mcp_servers"];
    for path in open_objects {
        let keys = requests
            .values()
            .flatten()
            .filter_map(|request| request.pointer(path))
            .map(|value| {
                value
                    .as_object()
                    .expect("an annotated map must serialize as an object")
            })
            .flat_map(|object| object.keys())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            keys.len() > 1,
            "map annotation {path} must cover different serialized keys"
        );
    }
    let contract = json!({
        "openObjects": open_objects,
        "requests": requests
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/acp/codex/sent-requests.json");
    if std::env::var_os("SAGE_UPDATE_CODEX_REQUEST_EVIDENCE").is_some() {
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&contract).unwrap()),
        )
        .unwrap();
    }
    let recorded: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        recorded, contract,
        "regenerate the sent-request contract after changing a serializer"
    );
}

fn reply(value: Value) -> crate::acp::transport::Frame {
    let bytes = format!("{value}\n");
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(bytes.as_bytes())
        .unwrap()
        .unwrap()
}
