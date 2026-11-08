use serde_json::{Value, json};

use super::*;
use crate::acp::transport::IncrementalFrameDecoder;

const LOGIN_ID: &str = "7be3f527-7a5a-41e5-9e5f-a1a74b42f86b";

fn protocol() -> CodexAuthProtocol {
    let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    CodexAuthProtocol::new(
        &packet,
        [9; 32],
        crate::acp::contract::RuntimeEpoch::new(73).unwrap(),
    )
}

#[test]
fn requests_match_the_pinned_codex_0146_wire_contract() {
    let mut protocol = protocol();
    let initialize = protocol.initialize_request().unwrap();
    let initialized = protocol
        .accept_initialize(&initialize_result(initialize.id()), initialize.id())
        .unwrap();
    assert_eq!(
        request_payload(initialized.payload()),
        json!({"method":"initialized"})
    );
    assert_eq!(
        request_value(protocol.account_request().unwrap()),
        json!({"id":2,"method":"account/read","params":{}})
    );
    assert_eq!(
        request_value(protocol.login_request().unwrap()),
        json!({"id":3,"method":"account/login/start","params":{
            "type":"chatgpt","appBrand":null
        }})
    );
    let login_id = NativeLoginId::parse(LOGIN_ID).unwrap();
    assert_eq!(
        request_value(protocol.cancel_request(&login_id).unwrap()),
        json!({"id":4,"method":"account/login/cancel","params":{"loginId":LOGIN_ID}})
    );
    assert_eq!(
        request_value(protocol.logout_request().unwrap()),
        json!({"id":5,"method":"account/logout"})
    );
}

#[test]
fn login_start_accepts_only_correlated_chatgpt_and_exact_authorization_origin() {
    let mut protocol = protocol();
    let request = protocol.login_request().unwrap();
    let (login, url) = protocol
        .accept_login_started(
            &frame(json!({"id":request.id(),"result":{
                "type":"chatgpt","loginId":LOGIN_ID,
                "authUrl":"https://auth.openai.com/oauth/authorize?client_id=sage&state=opaque"
            }})),
            request.id(),
        )
        .unwrap();
    assert_eq!(login.as_str(), LOGIN_ID);
    assert!(url.as_str().starts_with("https://auth.openai.com/"));

    for rejected in [
        "https://chatgpt.com/oauth/authorize?state=x",
        "https://auth.openai.com.evil.test/oauth/authorize?state=x",
        "https://auth.openai.com/oauth/other?state=x",
        "https://user@auth.openai.com/oauth/authorize?state=x",
        "https://auth.openai.com/oauth/authorize?state=x#secret",
        "http://auth.openai.com/oauth/authorize?state=x",
    ] {
        let request = protocol.login_request().unwrap();
        assert_eq!(
            protocol.accept_login_started(
                &frame(json!({"id":request.id(),"result":{
                    "type":"chatgpt","loginId":LOGIN_ID,"authUrl":rejected
                }})),
                request.id()
            ),
            Err(CodexAuthError::UrlPolicy)
        );
    }
}

#[test]
fn completion_and_account_updates_project_only_sanitized_state() {
    let protocol = protocol();
    assert_eq!(
        protocol.accept_notification(&frame(json!({
            "method":"account/login/completed","params":{
                "loginId":LOGIN_ID,"success":true,"error":null
            }
        }))),
        Ok(CodexAuthNotification::LoginCompleted {
            login_id: Some(NativeLoginId::parse(LOGIN_ID).unwrap()),
            success: true,
        })
    );
    assert_eq!(
        protocol.accept_notification(&frame(json!({
            "method":"account/updated","params":{"authMode":"chatgpt","planType":"plus"}
        }))),
        Ok(CodexAuthNotification::AccountUpdated {
            mode: Some(CodexAuthMode::Chatgpt)
        })
    );
    assert_eq!(
        protocol.accept_notification(&frame(json!({
            "method":"account/updated","params":{"authMode":null,"planType":null}
        }))),
        Ok(CodexAuthNotification::AccountUpdated { mode: None })
    );
}

#[test]
fn response_and_notification_shapes_are_closed_and_bounded() {
    let mut protocol = protocol();
    let login = protocol.login_request().unwrap();
    for rejected in [
        json!({"id":login.id()+1,"result":{"type":"chatgpt","loginId":LOGIN_ID,"authUrl":"https://auth.openai.com/oauth/authorize?x=1"}}),
        json!({"id":login.id(),"result":{"type":"chatgpt","loginId":"not-a-uuid","authUrl":"https://auth.openai.com/oauth/authorize?x=1"}}),
        json!({"id":login.id(),"result":{"type":"chatgpt","loginId":LOGIN_ID,"authUrl":"https://auth.openai.com/oauth/authorize?x=1","email":"private@example.test"}}),
    ] {
        assert!(
            protocol
                .accept_login_started(&frame(rejected), login.id())
                .is_err()
        );
    }
    assert!(
        protocol
            .accept_notification(&frame(json!({
                "method":"account/login/completed","params":{
                    "loginId":LOGIN_ID,"success":false,"error":"x".repeat(AUTH_ERROR_BYTES + 1)
                }
            })))
            .is_err()
    );
}

#[test]
fn cancel_and_logout_responses_are_exact() {
    let protocol = protocol();
    assert_eq!(
        protocol.accept_cancel_response(&frame(json!({"id":4,"result":{"status":"canceled"}})), 4),
        Ok(true)
    );
    assert_eq!(
        protocol.accept_cancel_response(&frame(json!({"id":5,"result":{"status":"notFound"}})), 5),
        Ok(false)
    );
    assert_eq!(
        protocol.accept_empty_response(&frame(json!({"id":6,"result":{}})), 6),
        Ok(())
    );
}

fn request_value(request: CodexAuthRequest) -> Value {
    request_payload(request.payload())
}

fn request_payload(payload: &[u8]) -> Value {
    serde_json::from_slice(payload).unwrap()
}

fn initialize_result(id: u64) -> Frame {
    frame(json!({"id":id,"result":{
        "userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) terminal (sage; 0.1.0)",
        "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
        "platformFamily":"windows","platformOs":"windows"
    }}))
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8 * 1024) {
        let candidate = decoder.push_chunk(chunk).unwrap();
        assert!(frame.is_none());
        frame = candidate;
    }
    frame.unwrap()
}
