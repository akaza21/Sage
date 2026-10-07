use std::{collections::VecDeque, time::Duration};

use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::codex::parse_packet_metadata, contract::RuntimeEpoch,
    transport::IncrementalFrameDecoder,
};

const LOGIN: &str = "7be3f527-7a5a-41e5-9e5f-a1a74b42f86b";
const FOREIGN: &str = "9a833e11-0fb0-46f8-a1b5-3cf48ffb2c89";

struct Browser {
    opened: bool,
}

impl AuthBrowser for std::sync::Mutex<Browser> {
    fn open(&self, url: &NativeAuthUrl) -> Result<(), CodexDiscoveryRunError> {
        assert!(url.as_str().starts_with("https://auth.openai.com/"));
        self.lock().unwrap().opened = true;
        Ok(())
    }
}

struct ScriptTransport {
    reads: VecDeque<Result<Option<Frame>, CodexDiscoveryRunError>>,
    writes: Vec<Value>,
    cancel_after_writes: usize,
    closed: bool,
    terminated_failed: Option<bool>,
    cleanup: Result<u64, CodexDiscoveryRunError>,
}

impl AuthTransport for ScriptTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.writes
            .push(serde_json::from_slice(frame.payload()).unwrap());
        FrameWriteOutcome::Complete
    }

    fn read_frame(&mut self, _: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        self.reads
            .pop_front()
            .unwrap_or(Err(CodexDiscoveryRunError::Timeout))
    }

    fn user_cancel_requested(&self) -> bool {
        self.writes.len() >= self.cancel_after_writes
    }

    fn close_stdin(&mut self) {
        self.closed = true;
    }

    fn terminate_zero(
        &mut self,
        failed: bool,
        _: CodexAuthDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        self.terminated_failed = Some(failed);
        self.cleanup.map(DiscoveryZeroProcess::fixture)
    }
}

#[test]
fn completion_and_update_may_arrive_before_start_response() {
    let mut transport = transport([
        initialize(1),
        signed_out(2),
        completed(LOGIN, true),
        updated(true),
        started(3),
    ]);
    let browser = std::sync::Mutex::new(Browser { opened: false });

    assert_eq!(
        run_login_protocol(
            &mut protocol(),
            &mut transport,
            &browser,
            73,
            CodexAuthDeadline::test()
        ),
        Ok(CodexAuthOutcome::SignedIn)
    );
    assert!(browser.lock().unwrap().opened);
    assert_settled(&transport, false);
}

#[test]
fn response_then_paired_notifications_completes_signed_in() {
    let mut transport = transport([
        initialize(1),
        signed_out(2),
        started(3),
        completed(LOGIN, true),
        updated(true),
    ]);
    let browser = std::sync::Mutex::new(Browser { opened: false });
    assert_eq!(
        run_login_protocol(
            &mut protocol(),
            &mut transport,
            &browser,
            73,
            CodexAuthDeadline::test()
        ),
        Ok(CodexAuthOutcome::SignedIn)
    );
    assert_settled(&transport, false);
}

#[test]
fn cancel_response_and_failed_completion_may_race_in_either_order() {
    for tail in [
        vec![cancelled(4), completed(LOGIN, false)],
        vec![completed(LOGIN, false), cancelled(4)],
    ] {
        let mut frames = vec![initialize(1), signed_out(2), started(3)];
        frames.extend(tail);
        let mut transport = transport_vec(frames);
        transport.cancel_after_writes = 4;
        assert_eq!(
            run_login_protocol(
                &mut protocol(),
                &mut transport,
                &std::sync::Mutex::new(Browser { opened: false }),
                73,
                CodexAuthDeadline::test()
            ),
            Ok(CodexAuthOutcome::Cancelled)
        );
        assert_eq!(transport.writes[4]["method"], "account/login/cancel");
        assert_settled(&transport, false);
    }
}

#[test]
fn logout_accepts_canceled_completion_interleaved_before_null_update() {
    let mut transport = transport([
        initialize(1),
        completed(LOGIN, false),
        json_frame(json!({"id":2,"result":{}})),
        updated(false),
    ]);
    assert_eq!(
        run_logout_protocol(
            &mut protocol(),
            &mut transport,
            73,
            CodexAuthDeadline::test()
        ),
        Ok(CodexAuthOutcome::SignedOut)
    );
    assert_eq!(
        transport.writes[2],
        json!({"id":2,"method":"account/logout"})
    );
    assert_settled(&transport, false);
}

#[test]
fn account_observation_recovers_effect_without_replaying_a_mutation() {
    for (account, expected) in [
        (signed_out(2), CodexAuthOutcome::SignedOut),
        (signed_in(2), CodexAuthOutcome::SignedIn),
    ] {
        let mut transport = transport([initialize(1), account]);
        assert_eq!(
            run_account_protocol(
                &mut protocol(),
                &mut transport,
                73,
                CodexAuthDeadline::test()
            ),
            Ok(expected)
        );
        assert_eq!(transport.writes[2]["method"], "account/read");
        assert_settled(&transport, false);
    }
}

#[test]
fn account_updated_may_arrive_before_account_read_response() {
    let mut observed = transport([initialize(1), updated(true), signed_in(2)]);
    assert_eq!(
        run_account_protocol(
            &mut protocol(),
            &mut observed,
            73,
            CodexAuthDeadline::test()
        ),
        Ok(CodexAuthOutcome::SignedIn)
    );
    assert_settled(&observed, false);

    let mut login = transport([
        initialize(1),
        updated(false),
        signed_out(2),
        started(3),
        completed(LOGIN, true),
        updated(true),
    ]);
    assert_eq!(
        run_login_protocol(
            &mut protocol(),
            &mut login,
            &std::sync::Mutex::new(Browser { opened: false }),
            73,
            CodexAuthDeadline::test()
        ),
        Ok(CodexAuthOutcome::SignedIn)
    );
    assert_settled(&login, false);
}

#[test]
fn early_account_update_must_match_the_bounded_account_response() {
    for frames in [
        vec![initialize(1), updated(true), signed_out(2)],
        vec![initialize(1), updated(false), signed_in(2)],
        vec![initialize(1), updated(true), updated(true), signed_in(2)],
        vec![initialize(1), completed(LOGIN, false), signed_out(2)],
    ] {
        let mut transport = transport_vec(frames);
        assert_eq!(
            run_account_protocol(
                &mut protocol(),
                &mut transport,
                73,
                CodexAuthDeadline::test()
            ),
            Err(CodexDiscoveryRunError::Protocol)
        );
        assert_settled(&transport, true);
    }
}

#[test]
fn wrong_ids_login_identity_methods_eof_timeout_and_cleanup_fail_closed() {
    let cases = [
        vec![initialize(1), signed_out(2), started_with(9, LOGIN)],
        vec![
            initialize(1),
            signed_out(2),
            started(3),
            completed(FOREIGN, true),
        ],
        vec![initialize(1), signed_out(2), started(3), updated(false)],
    ];
    for frames in cases {
        let mut transport = transport_vec(frames);
        assert_eq!(
            run_login_protocol(
                &mut protocol(),
                &mut transport,
                &std::sync::Mutex::new(Browser { opened: false }),
                73,
                CodexAuthDeadline::test()
            ),
            Err(CodexDiscoveryRunError::Protocol)
        );
        assert_settled(&transport, true);
    }

    for failure in [CodexDiscoveryRunError::Eof, CodexDiscoveryRunError::Timeout] {
        let mut transport = transport([initialize(1), signed_out(2), started(3)]);
        transport.reads.push_back(match failure {
            CodexDiscoveryRunError::Eof => Ok(None),
            error => Err(error),
        });
        assert_eq!(
            run_login_protocol(
                &mut protocol(),
                &mut transport,
                &std::sync::Mutex::new(Browser { opened: false }),
                73,
                CodexAuthDeadline::test()
            ),
            Err(failure)
        );
        assert_settled(&transport, true);
    }

    let mut cleanup = transport([
        initialize(1),
        signed_out(2),
        started(3),
        completed(LOGIN, true),
        updated(true),
    ]);
    cleanup.cleanup = Err(CodexDiscoveryRunError::Cleanup);
    assert_eq!(
        run_login_protocol(
            &mut protocol(),
            &mut cleanup,
            &std::sync::Mutex::new(Browser { opened: false }),
            73,
            CodexAuthDeadline::test()
        ),
        Err(CodexDiscoveryRunError::Cleanup)
    );
}

fn protocol() -> CodexAuthProtocol {
    let packet = parse_packet_metadata(include_bytes!(
        "../../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    CodexAuthProtocol::new(&packet, [9; 32], RuntimeEpoch::new(73).unwrap())
}

fn transport<const N: usize>(frames: [Frame; N]) -> ScriptTransport {
    transport_vec(frames.into_iter().collect())
}

fn transport_vec(frames: Vec<Frame>) -> ScriptTransport {
    ScriptTransport {
        reads: frames.into_iter().map(|frame| Ok(Some(frame))).collect(),
        writes: Vec::new(),
        cancel_after_writes: usize::MAX,
        closed: false,
        terminated_failed: None,
        cleanup: Ok(73),
    }
}

fn initialize(id: u64) -> Frame {
    json_frame(json!({"id":id,"result":{
        "userAgent":"sage/0.145.0 (Windows 10.0.26200; x86_64) terminal (sage; 0.1.0)",
        "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.145.0-7b0ab167",
        "platformFamily":"windows","platformOs":"windows"
    }}))
}

fn signed_out(id: u64) -> Frame {
    json_frame(json!({"id":id,"result":{"account":null,"requiresOpenaiAuth":true}}))
}

fn signed_in(id: u64) -> Frame {
    json_frame(json!({"id":id,"result":{
        "account":{"type":"chatgpt","email":"private@example.test","planType":"plus"},
        "requiresOpenaiAuth":true
    }}))
}

fn started(id: u64) -> Frame {
    started_with(id, LOGIN)
}

fn started_with(id: u64, login: &str) -> Frame {
    json_frame(json!({"id":id,"result":{
        "type":"chatgpt","loginId":login,
        "authUrl":"https://auth.openai.com/oauth/authorize?client_id=sage&state=opaque"
    }}))
}

fn completed(login: &str, success: bool) -> Frame {
    json_frame(json!({"method":"account/login/completed","params":{
        "loginId":login,"success":success,"error":if success { Value::Null } else { json!("cancelled") }
    }}))
}

fn updated(signed_in: bool) -> Frame {
    json_frame(if signed_in {
        json!({"method":"account/updated","params":{"authMode":"chatgpt","planType":"plus"}})
    } else {
        json!({"method":"account/updated","params":{"authMode":null,"planType":null}})
    })
}

fn cancelled(id: u64) -> Frame {
    json_frame(json!({"id":id,"result":{"status":"canceled"}}))
}

fn json_frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    decoder.push_chunk(&bytes).unwrap().unwrap()
}

fn assert_settled(transport: &ScriptTransport, failed: bool) {
    assert!(transport.closed);
    assert_eq!(transport.terminated_failed, Some(failed));
}
