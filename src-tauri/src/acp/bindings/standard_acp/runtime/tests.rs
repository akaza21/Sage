use std::{path::PathBuf, sync::Arc};

use serde_json::{Value, json};

use crate::{
    acp::{
        bindings::{
            adapter::{
                AdapterCommand, AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressResolution,
                HarnessAdapter, ResourceMapper,
            },
            runtime::{NativeRuntime, RuntimeFactory},
        },
        contract::{
            ProjectGeneration, RequiredText, RuntimeEpoch, SageAgentSessionId, SageProjectId,
            SageRootId, SageServerRequestId,
        },
        intent::{NormalizedPayload, Prompt},
        lifecycle::TurnState,
        transport::{Frame, IncrementalFrameDecoder},
    },
    acp_protocol::{access_mode::SessionAccessMode, permissions::PermissionDecision},
    project_registry::ProjectRootAuthority,
    settings_store::CustomAcpAgent,
};

use super::super::{CustomAcpRegistration, StandardAcpBindingSource, StandardAcpRuntimeFactory};

const SESSION: &str = "agent-session-1";

struct Fixture {
    root: PathBuf,
    registration: Arc<CustomAcpRegistration>,
    runtime: NativeRuntime,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).expect("json");
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .expect("frame")
        .expect("complete frame")
}

fn wire(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).expect("egress json")
}

fn written(runtime: &mut NativeRuntime, egress: AdapterEgress) -> Value {
    let value = wire(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .expect("written");
    value
}

fn custom_runtime(label: &str) -> Fixture {
    let root = sage_test_support::test_paths::temp_dir().join(format!(
        "sage-custom-runtime-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("root");
    let root = root.canonicalize().expect("canonical root");
    let executable = root.join("agent.exe");
    crate::acp::bindings::standard_acp::custom_executable::write_test_executable(
        &executable,
        b"scripted agent",
    );
    let registration = Arc::new(CustomAcpRegistration::startup(1).expect("slot"));
    registration.apply(Some(&CustomAcpAgent {
        id: "custom-acp-1".into(),
        name: "Scripted agent".into(),
        executable: executable.to_string_lossy().into_owned(),
        args: Vec::new(),
        env: Vec::new(),
    }));
    let binding = registration.admitted().expect("admitted");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse(format!("project-{label}")).unwrap(),
        ProjectGeneration::ZERO,
        SageRootId::parse(format!("root-{label}")).unwrap(),
        &root,
    )
    .expect("project root");
    let runtime = Box::new(StandardAcpRuntimeFactory::from_admitted_binding(
        Arc::clone(&registration) as _,
        binding,
        project_root,
    ))
    .construct(
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse(format!("session-{label}")).unwrap(),
    )
    .expect("runtime");
    Fixture {
        root,
        registration,
        runtime,
    }
}

fn initialize(fixture: &mut Fixture, agent_capabilities: Value, auth_methods: Value) -> Value {
    let request = fixture
        .runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    let request = written(&mut fixture.runtime, request);
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 1, "result": {
                "protocolVersion": 1,
                "agentCapabilities": agent_capabilities,
                "authMethods": auth_methods
            }})),
            &Mapper,
        )
        .expect("initialize response");
    assert!(matches!(batch.events(), [AdapterEvent::Initialized { .. }]));
    request
}

fn open_session(fixture: &mut Fixture) -> Value {
    let request = fixture
        .runtime
        .command(AdapterCommand::NewSession {
            native_root: fixture.root.to_string_lossy().into_owned(),
            model: None,
        })
        .expect("new session");
    written(&mut fixture.runtime, request)
}

fn ready(fixture: &mut Fixture, id: u64) {
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": SESSION}})),
            &Mapper,
        )
        .expect("session");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { loaded: false, .. }]
    ));
}

fn start_turn(fixture: &mut Fixture) -> u64 {
    let request = fixture
        .runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Hello agent").unwrap(),
            client_message_id: RequiredText::new("client", "message-1").unwrap(),
            access_mode: SessionAccessMode::Ask,
        })
        .expect("prompt");
    let value = written(&mut fixture.runtime, request);
    assert_eq!(value["method"], "session/prompt");
    value["id"].as_u64().unwrap()
}

fn running(label: &str) -> Fixture {
    let mut fixture = custom_runtime(label);
    initialize(&mut fixture, json!({}), json!([]));
    open_session(&mut fixture);
    ready(&mut fixture, 2);
    fixture
}

fn elicit(fixture: &mut Fixture, params: Value) -> (SageServerRequestId, NormalizedPayload) {
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 90, "method": "elicitation/create", "params": params})),
            &Mapper,
        )
        .expect("elicitation");
    let [
        AdapterEvent::PermissionRequest {
            external_request_id,
            payload,
            ..
        },
    ] = batch.events()
    else {
        panic!("one question card, got {:?}", batch.events());
    };
    assert!(batch.egress().is_empty(), "the agent waits for the user");
    (
        SageServerRequestId::parse(external_request_id.as_str()).unwrap(),
        payload.clone(),
    )
}

fn form_params() -> Value {
    json!({"sessionId": SESSION, "mode": "form", "message": "Which branch?",
        "requestedSchema": {"type": "object", "properties": {
            "branch": {"type": "string", "title": "Branch",
                "oneOf": [{"const": "main", "title": "main"}, {"const": "dev", "title": "dev"}]}
        }, "required": ["branch"]}})
}

#[test]
fn initialize_advertises_what_sage_implements_and_reads_the_agent() {
    let mut fixture = custom_runtime("initialize");
    let request = initialize(
        &mut fixture,
        json!({"promptCapabilities": {"embeddedContext": true}, "sessionCapabilities": {"close": {}}}),
        json!([]),
    );
    assert_eq!(
        request["params"]["clientCapabilities"],
        json!({
            "fs": {"readTextFile": false, "writeTextFile": true},
            "terminal": false,
            "elicitation": {"form": {}, "url": {}}
        })
    );
    let capabilities = fixture.runtime.capabilities().expect("capabilities");
    assert!(capabilities.prompt_resources);
    assert!(capabilities.close_session);

    let mut silent = custom_runtime("initialize-silent");
    initialize(&mut silent, json!({}), json!([]));
    let capabilities = silent.runtime.capabilities().expect("capabilities");
    assert!(!capabilities.prompt_resources);
    assert!(
        !capabilities.close_session,
        "an agent that does not advertise close is never sent session/close"
    );
}

#[test]
fn a_plain_turn_runs_without_any_mode_switch() {
    let mut fixture = running("plain");
    let prompt = start_turn(&mut fixture);
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": SESSION,
                "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Hi there"}}}})),
            &Mapper,
        )
        .expect("chunk");
    assert!(
        batch
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::Item { .. }))
    );
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": prompt, "result": {"stopReason": "end_turn"}})),
            &Mapper,
        )
        .expect("terminal");
    let AdapterEvent::Turn { id, state } = batch.events().last().expect("turn") else {
        panic!("terminal turn event");
    };
    assert_eq!(*state, TurnState::Completed);
    assert_eq!(
        id.turn_id.as_ref().unwrap().as_str(),
        format!("custom-acp-slot-1-prompt-{prompt}")
    );
}

#[test]
fn a_form_elicitation_is_answered_with_the_typed_content() {
    let mut fixture = running("form");
    start_turn(&mut fixture);
    let (request, payload) = elicit(&mut fixture, form_params());
    let NormalizedPayload::PermissionRequest { user_input, .. } = payload else {
        panic!("question payload");
    };
    assert_eq!(user_input.expect("form").questions.as_slice().len(), 1);
    let answers =
        serde_json::from_value(json!([{"questionId": "branch", "selectedOption": "dev"}])).unwrap();
    let reply = fixture
        .runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request.clone(),
            decision: PermissionDecision::Answer(answers),
        })
        .expect("answer");
    assert!(reply.token().allow_request().is_some());
    assert_eq!(
        written(&mut fixture.runtime, reply),
        json!({"jsonrpc": "2.0", "id": 90, "result": {"action": "accept", "content": {"branch": "dev"}}})
    );
    assert!(
        fixture
            .runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: request,
                decision: PermissionDecision::Deny,
            })
            .is_err(),
        "an answered question cannot be answered again"
    );
}

#[test]
fn a_url_elicitation_is_accepted_without_content_and_a_deny_declines() {
    let mut fixture = running("url");
    start_turn(&mut fixture);
    let (request, _) = elicit(
        &mut fixture,
        json!({"sessionId": SESSION, "mode": "url", "elicitationId": "e1",
            "message": "Sign in to the docs server", "url": "https://auth.example.com/authorize"}),
    );
    let none = serde_json::from_value(json!([])).unwrap();
    let reply = fixture
        .runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: PermissionDecision::Answer(none),
        })
        .expect("accept");
    assert_eq!(
        written(&mut fixture.runtime, reply)["result"],
        json!({"action": "accept"})
    );

    let mut declined = running("decline");
    start_turn(&mut declined);
    let (request, _) = elicit(&mut declined, form_params());
    let reply = declined
        .runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: request,
            decision: PermissionDecision::Deny,
        })
        .expect("deny");
    assert!(reply.token().deny_request().is_some());
    declined
        .runtime
        .commit_mandatory_egress(reply.token())
        .expect("commit decline");
    assert_eq!(
        written(&mut declined.runtime, reply)["result"],
        json!({"action": "decline"})
    );
}

#[test]
fn a_cancelled_turn_cancels_its_pending_elicitation() {
    let mut fixture = running("cancel");
    start_turn(&mut fixture);
    let (request, _) = elicit(&mut fixture, form_params());
    let reply = fixture
        .runtime
        .cancel_permission(&request)
        .expect("cancel reply");
    assert!(reply.token().cancelled_permission_request().is_some());
    fixture
        .runtime
        .commit_mandatory_egress(reply.token())
        .expect("commit cancel");
    assert_eq!(
        written(&mut fixture.runtime, reply)["result"],
        json!({"action": "cancel"})
    );
}

#[test]
fn an_elicitation_outside_a_turn_or_for_another_session_is_cancelled_at_once() {
    let mut fixture = running("idle");
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 91, "method": "elicitation/create", "params": form_params()})),
            &Mapper,
        )
        .expect("idle elicitation");
    assert!(batch.events().is_empty());
    assert_eq!(
        wire(&batch.egress()[0])["result"],
        json!({"action": "cancel"})
    );

    start_turn(&mut fixture);
    let mut foreign = form_params();
    foreign["sessionId"] = json!("another-session");
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 92, "method": "elicitation/create", "params": foreign})),
            &Mapper,
        )
        .expect("foreign elicitation");
    assert!(batch.events().is_empty());
    assert_eq!(
        wire(&batch.egress()[0])["result"],
        json!({"action": "cancel"})
    );
}

#[test]
fn sign_in_without_input_authenticates_and_opens_the_session_again() {
    let mut fixture = custom_runtime("auth");
    initialize(
        &mut fixture,
        json!({}),
        json!([{"id": "browser", "name": "Browser sign-in"}]),
    );
    open_session(&mut fixture);
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32000, "message": "Authentication required"}})),
            &Mapper,
        )
        .expect("refusal leads to authenticate");
    let Ok([authenticate]) = <[AdapterEgress; 1]>::try_from(batch.into_parts().1) else {
        panic!("one authenticate request");
    };
    assert_eq!(
        written(&mut fixture.runtime, authenticate),
        json!({"jsonrpc": "2.0", "id": 3, "method": "authenticate", "params": {"methodId": "browser"}})
    );
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 3, "result": {}})),
            &Mapper,
        )
        .expect("authenticated");
    let Ok([reopen]) = <[AdapterEgress; 1]>::try_from(batch.into_parts().1) else {
        panic!("session/new again");
    };
    let reopen = written(&mut fixture.runtime, reopen);
    assert_eq!(reopen["method"], "session/new");
    assert_eq!(reopen["id"], 4);
    ready(&mut fixture, 4);
}

#[test]
fn sign_in_that_needs_the_user_is_a_notice_naming_each_method() {
    let mut fixture = custom_runtime("auth-notice");
    initialize(
        &mut fixture,
        json!({}),
        json!([{"id": "term", "name": "Terminal login", "type": "terminal", "args": ["login"]}]),
    );
    open_session(&mut fixture);
    let refused = fixture.runtime.ingest(
        &frame(json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32000, "message": "Authentication required"}})),
        &Mapper,
    );
    assert!(matches!(refused, Err(AdapterRuntimeError::SignInRequired)));
    let detail = fixture.runtime.refusal_detail().expect("notice");
    assert_eq!(
        detail.message(),
        Some(
            "The agent needs sign-in. It offers: Terminal login (run the agent in a terminal with login)."
        )
    );
}

#[test]
fn an_opened_session_clears_a_sign_in_notice_left_by_an_earlier_refusal() {
    let mut fixture = custom_runtime("auth-cleared");
    fixture
        .registration
        .record_sign_in("Scripted agent needs sign-in. It offers: Terminal login.");
    assert!(fixture.registration.sign_in_notice().is_some());
    initialize(&mut fixture, json!({}), json!([]));
    open_session(&mut fixture);
    ready(&mut fixture, 2);
    assert_eq!(fixture.registration.sign_in_notice(), None);
}

#[test]
fn an_agent_that_refuses_after_authenticating_is_not_told_sage_signs_in() {
    let mut fixture = custom_runtime("auth-again");
    initialize(
        &mut fixture,
        json!({}),
        json!([{"id": "browser", "name": "Browser sign-in"}]),
    );
    open_session(&mut fixture);
    let refusal = json!({"code": -32000, "message": "Authentication required"});
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 2, "error": refusal})),
            &Mapper,
        )
        .expect("authenticate");
    let authenticate = batch.into_parts().1.into_iter().next().expect("egress");
    written(&mut fixture.runtime, authenticate);
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 3, "result": {}})),
            &Mapper,
        )
        .expect("reopen");
    let reopen = batch
        .into_parts()
        .1
        .into_iter()
        .next()
        .expect("session/new");
    written(&mut fixture.runtime, reopen);
    let refused = fixture.runtime.ingest(
        &frame(json!({"jsonrpc": "2.0", "id": 4, "error": refusal})),
        &Mapper,
    );
    assert!(matches!(refused, Err(AdapterRuntimeError::SignInRequired)));
    let expected = "The agent needs sign-in. It offers: Browser sign-in (Sage tried it and the agent did not accept it; sign in outside Sage).";
    assert_eq!(
        fixture.runtime.refusal_detail().expect("notice").message(),
        Some(expected)
    );
    assert_eq!(
        fixture.registration.sign_in_notice().as_deref(),
        Some(expected)
    );
}

#[test]
fn a_failed_authenticate_is_a_sign_in_notice_without_a_second_attempt() {
    let mut fixture = custom_runtime("auth-failed");
    initialize(
        &mut fixture,
        json!({}),
        json!([{"id": "browser", "name": "Browser sign-in"}]),
    );
    open_session(&mut fixture);
    let batch = fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32000, "message": "Authentication required"}})),
            &Mapper,
        )
        .expect("authenticate");
    let authenticate = batch.into_parts().1.into_iter().next().expect("egress");
    written(&mut fixture.runtime, authenticate);
    let failed = fixture.runtime.ingest(
        &frame(json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32603, "message": "browser closed"}})),
        &Mapper,
    );
    assert!(matches!(failed, Err(AdapterRuntimeError::SignInRequired)));
    let notice = fixture.registration.sign_in_notice().expect("notice");
    assert!(!notice.contains("Sage signs in for you"), "{notice}");
}
