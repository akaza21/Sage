use super::*;
use crate::acp::browser::{BrowserScope, FakeBrowserCapability};
use serde_json::json;
use std::collections::VecDeque;

#[cfg(windows)]
#[test]
fn sage_helper_process_answers_initialize_over_bound_pipe() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let Some(binary) = std::env::var_os("SAGE_BROWSER_HELPER_TEST_BIN") else {
        return;
    };
    let host = super::BrowserMcpPipeHost::bind().unwrap();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        now_ms().unwrap(),
    );
    let environment = host.child_environment("session-1", token);
    let live = Arc::new(AtomicBool::new(true));
    let worker_live = Arc::clone(&live);
    let worker = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || {
            if worker_live.load(Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    let mut command = Command::new(binary);
    if std::env::var_os("SAGE_BROWSER_HELPER_TEST_CLEAR_ENV").is_some() {
        command.env_clear();
    }
    let mut child = command
        .arg(HELPER_ARGUMENT)
        .envs(environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(delay) = std::env::var("SAGE_BROWSER_HELPER_TEST_DELAY_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "helper stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: serde_json::Value =
        serde_json::from_slice(output.stdout.split(|byte| *byte == b'\n').next().unwrap()).unwrap();
    assert_eq!(response["result"]["serverInfo"]["name"], "sage-browser");
    live.store(false, Ordering::SeqCst);
    assert_eq!(worker.join().unwrap(), Err(BrowserMcpError::Unavailable));
}

#[cfg(windows)]
#[test]
fn sage_helper_reconnects_after_codex_restarts_it() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let Some(binary) = std::env::var_os("SAGE_BROWSER_HELPER_TEST_BIN") else {
        return;
    };
    let host = super::BrowserMcpPipeHost::bind().unwrap();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-restart".into(),
        now_ms().unwrap(),
    );
    let environment = host.child_environment("session-restart", token);
    let live = Arc::new(AtomicBool::new(true));
    let worker_live = Arc::clone(&live);
    let worker = std::thread::spawn(move || {
        host.serve(&mut server, "session-restart", || {
            if worker_live.load(Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    let run_helper = || {
        let mut child = Command::new(&binary)
            .arg(HELPER_ARGUMENT)
            .envs(environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n")
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let first = run_helper();
    let second = run_helper();
    live.store(false, Ordering::SeqCst);
    let _ = worker.join();
    assert!(first.status.success());
    assert!(
        second.status.success(),
        "second helper stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        String::from_utf8_lossy(&second.stdout).contains("sage-browser"),
        "second helper must answer initialize"
    );
}

#[derive(Default)]
struct FakePipe {
    incoming: VecDeque<Vec<u8>>,
    outgoing: Vec<Vec<u8>>,
}

impl MessagePipe for FakePipe {
    fn read(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, BrowserMcpError> {
        let frame = self
            .incoming
            .pop_front()
            .ok_or(BrowserMcpError::Unavailable)?;
        if frame.len() > maximum {
            return Err(BrowserMcpError::Capacity);
        }
        Ok(Some(frame))
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), BrowserMcpError> {
        self.outgoing.push(bytes.to_vec());
        Ok(())
    }
}

fn server() -> (BrowserMcpServer<FakeBrowserCapability>, BrowserMcpToken) {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    };
    let mut port = FakeBrowserCapability::default();
    port.open_tab(scope.clone());
    BrowserMcpServer::issue(port, scope, now_ms().unwrap())
}

fn hello(session_id: &str, token: BrowserMcpToken) -> Vec<u8> {
    serde_json::to_vec(&Handshake {
        session_id: session_id.into(),
        token: token.into_env_value(),
    })
    .unwrap()
}

#[test]
fn fake_pipe_authenticates_then_binds_one_session() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    pipe.incoming.push_back(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .unwrap(),
    );
    serve_authenticated(&mut server, &mut pipe, "session-1", || true).unwrap();
    assert_eq!(pipe.outgoing[0], b"OK");
    assert!(String::from_utf8_lossy(&pipe.outgoing[1]).contains("snapshotPage"));
}

#[test]
fn bad_scope_call_returns_error_and_next_call_uses_same_pipe() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    for (id, scope) in [
        (41, json!({"sessionId":"wrong-session","tabId":7})),
        (42, json!({"tabId":7})),
    ] {
        pipe.incoming.push_back(
            serde_json::to_vec(&json!({
                "jsonrpc":"2.0","id":id,"method":"tools/call",
                "params":{"name":"snapshotPage","arguments":{"scope":scope}}
            }))
            .unwrap(),
        );
    }
    serve_authenticated(&mut server, &mut pipe, "session-1", || true).unwrap();
    assert_eq!(pipe.outgoing.len(), 3);
    assert_eq!(pipe.outgoing[0], b"OK");
    let bad: serde_json::Value = serde_json::from_slice(&pipe.outgoing[1]).unwrap();
    let good: serde_json::Value = serde_json::from_slice(&pipe.outgoing[2]).unwrap();
    assert_eq!(bad["id"], 41);
    assert_eq!(bad["result"]["isError"], true);
    assert_eq!(good["id"], 42);
    assert_eq!(good["result"]["isError"], false);
}

#[test]
fn completed_call_cancellation_and_unknown_notification_keep_pipe_open() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    let call = |id| {
        json!({
            "jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":"snapshotPage","arguments":{"scope":{"tabId":7}}}
        })
    };
    for frame in [
        call(51),
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":51,"reason":"completed"}}),
        json!({"jsonrpc":"2.0","method":"notifications/unknown","params":{}}),
        call(52),
    ] {
        pipe.incoming.push_back(serde_json::to_vec(&frame).unwrap());
    }
    serve_authenticated(&mut server, &mut pipe, "session-1", || true).unwrap();
    assert_eq!(pipe.outgoing.len(), 5);
    assert_eq!(pipe.outgoing[0], b"OK");
    let first: serde_json::Value = serde_json::from_slice(&pipe.outgoing[1]).unwrap();
    assert_eq!(first["id"], 51);
    assert_eq!(first["result"]["isError"], false);
    assert_eq!(pipe.outgoing[2], NO_REPLY);
    assert_eq!(pipe.outgoing[3], NO_REPLY);
    let second: serde_json::Value = serde_json::from_slice(&pipe.outgoing[4]).unwrap();
    assert_eq!(second["id"], 52);
    assert_eq!(second["result"]["isError"], false);
}

#[test]
fn unknown_method_and_invalid_list_params_reply_without_closing_pipe() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    for frame in [
        json!({"jsonrpc":"2.0","id":61,"method":"not/a-browser-method","params":{}}),
        json!({"jsonrpc":"2.0","id":62,"method":"tools/list","params":{"unexpected":true}}),
        json!({"jsonrpc":"2.0","id":63,"method":"tools/call","params":{"name":"snapshotPage","arguments":{"scope":{"tabId":7}}}}),
    ] {
        pipe.incoming.push_back(serde_json::to_vec(&frame).unwrap());
    }
    serve_authenticated(&mut server, &mut pipe, "session-1", || true).unwrap();
    assert_eq!(pipe.outgoing.len(), 4);
    let unknown: serde_json::Value = serde_json::from_slice(&pipe.outgoing[1]).unwrap();
    assert_eq!(unknown["id"], 61);
    assert_eq!(unknown["error"]["code"], -32601);
    let invalid: serde_json::Value = serde_json::from_slice(&pipe.outgoing[2]).unwrap();
    assert_eq!(invalid["id"], 62);
    assert_eq!(invalid["error"]["code"], -32602);
    let valid: serde_json::Value = serde_json::from_slice(&pipe.outgoing[3]).unwrap();
    assert_eq!(valid["id"], 63);
    assert_eq!(valid["result"]["isError"], false);
}

#[test]
fn unavailable_tool_result_is_same_id_error_and_next_call_uses_same_pipe() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    for frame in [
        json!({"jsonrpc":"2.0","id":71,"method":"tools/call","params":{"name":"screenshot","arguments":{"scope":{"tabId":7},"region":null}}}),
        json!({"jsonrpc":"2.0","id":72,"method":"tools/call","params":{"name":"snapshotPage","arguments":{"scope":{"tabId":7}}}}),
    ] {
        pipe.incoming.push_back(serde_json::to_vec(&frame).unwrap());
    }
    serve_authenticated(&mut server, &mut pipe, "session-1", || true).unwrap();
    assert_eq!(pipe.outgoing.len(), 3);
    let failed: serde_json::Value = serde_json::from_slice(&pipe.outgoing[1]).unwrap();
    assert_eq!(failed["id"], 71);
    assert_eq!(failed["result"]["isError"], true);
    let next: serde_json::Value = serde_json::from_slice(&pipe.outgoing[2]).unwrap();
    assert_eq!(next["id"], 72);
    assert_eq!(next["result"]["isError"], false);
}

#[test]
fn fake_pipe_rejects_wrong_token_and_closed_session() {
    let (_, wrong) = server();
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", wrong));
    assert_eq!(
        serve_authenticated(&mut server, &mut pipe, "session-1", || true),
        Err(BrowserMcpError::Unauthorized)
    );
    let mut closed = FakePipe::default();
    closed.incoming.push_back(hello("session-1", token));
    assert_eq!(
        serve_authenticated(&mut server, &mut closed, "session-1", || false),
        Err(BrowserMcpError::Unavailable)
    );
}

#[test]
fn closed_session_after_handshake_rejects_the_next_request() {
    let (mut server, token) = server();
    let mut pipe = FakePipe::default();
    pipe.incoming.push_back(hello("session-1", token));
    pipe.incoming.push_back(
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .unwrap(),
    );
    let checks = std::cell::Cell::new(0);
    assert_eq!(
        serve_authenticated(&mut server, &mut pipe, "session-1", || {
            checks.set(checks.get() + 1);
            checks.get() < 4
        }),
        Err(BrowserMcpError::Unavailable)
    );
    assert_eq!(pipe.outgoing, vec![b"OK".to_vec()]);
}

#[test]
fn closing_session_while_pipe_is_idle_releases_the_host() {
    struct IdleClosingPipe {
        hello: Option<Vec<u8>>,
        live: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl MessagePipe for IdleClosingPipe {
        fn read(&mut self, _: usize) -> Result<Option<Vec<u8>>, BrowserMcpError> {
            if let Some(hello) = self.hello.take() {
                return Ok(Some(hello));
            }
            self.live.set(false);
            Ok(None)
        }
        fn write(&mut self, _: &[u8]) -> Result<(), BrowserMcpError> {
            Ok(())
        }
    }
    let (mut server, token) = server();
    let live = std::rc::Rc::new(std::cell::Cell::new(true));
    let mut pipe = IdleClosingPipe {
        hello: Some(hello("session-1", token)),
        live: live.clone(),
    };
    assert_eq!(
        serve_authenticated(&mut server, &mut pipe, "session-1", || live.get()),
        Err(BrowserMcpError::Unavailable)
    );
}
