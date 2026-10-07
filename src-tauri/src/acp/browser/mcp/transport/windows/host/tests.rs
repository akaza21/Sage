use super::*;
use crate::acp::browser::{
    BrowserFailure, BrowserRequest, BrowserResult, BrowserScope, FakeBrowserCapability,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

fn start<P: BrowserCapability + Send + 'static>(
    port: P,
) -> (
    String,
    String,
    Arc<AtomicBool>,
    std::thread::JoinHandle<Result<(), BrowserMcpError>>,
) {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        port,
        "session-reconnect".into(),
        super::super::super::now_ms().unwrap(),
    );
    let live = Arc::new(AtomicBool::new(true));
    let worker_live = Arc::clone(&live);
    let worker = std::thread::spawn(move || {
        host.serve(&mut server, "session-reconnect", || {
            if worker_live.load(Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    (name, token.into_env_value(), live, worker)
}

fn connect(name: &str) -> Pipe {
    connect_client(
        name,
        std::process::id(),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap()
}

fn read_frame(pipe: &mut Pipe) -> Result<Vec<u8>, BrowserMcpError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(frame) = pipe.read(BROWSER_LIMITS.result_bytes)? {
            return Ok(frame);
        }
    }
    Err(BrowserMcpError::Unavailable)
}

fn send_hello(pipe: &mut Pipe, token: &str) -> Result<Vec<u8>, BrowserMcpError> {
    pipe.write(
        &serde_json::to_vec(&json!({"session_id":"session-reconnect","token":token})).unwrap(),
    )?;
    read_frame(pipe)
}

fn initialize(pipe: &mut Pipe) -> Result<serde_json::Value, BrowserMcpError> {
    pipe.write(
        &serde_json::to_vec(&json!({
            "jsonrpc":"2.0","id":1,"method":"initialize","params":{}
        }))
        .unwrap(),
    )?;
    Ok(serde_json::from_slice(&read_frame(pipe)?).unwrap())
}

fn stop(live: Arc<AtomicBool>, worker: std::thread::JoinHandle<Result<(), BrowserMcpError>>) {
    live.store(false, Ordering::SeqCst);
    assert_eq!(worker.join().unwrap(), Err(BrowserMcpError::Unavailable));
}

#[test]
fn listener_creation_failure_preserves_authenticated_client() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-reconnect".into(),
        super::super::super::now_ms().unwrap(),
    );
    let live = Arc::new(AtomicBool::new(true));
    let worker_live = Arc::clone(&live);
    let worker = std::thread::spawn(move || {
        let session_state = || {
            if worker_live.load(Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        };
        accept(&host.pipe, &session_state)?;
        serve_connected(
            host,
            &mut server,
            "session-reconnect",
            &session_state,
            &|_| Err(BrowserMcpError::Unavailable),
        )
    });
    let mut client = connect(&name);
    assert_eq!(
        send_hello(&mut client, &token.into_env_value()).unwrap(),
        b"OK"
    );
    assert_eq!(
        initialize(&mut client).unwrap()["result"]["serverInfo"]["name"],
        "sage-browser"
    );
    client
        .write(
            &serde_json::to_vec(&json!({
                "jsonrpc":"2.0","id":2,"method":"tools/list","params":{}
            }))
            .unwrap(),
        )
        .unwrap();
    let response: serde_json::Value =
        serde_json::from_slice(&read_frame(&mut client).unwrap()).unwrap();
    assert_eq!(response["id"], 2);
    assert!(!response["result"]["tools"].as_array().unwrap().is_empty());
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(150)
        )
        .is_err()
    );
    stop(live, worker);
}

#[test]
fn four_authenticated_connections_are_allowed_but_fifth_is_rejected() {
    let (name, token, live, worker) = start(FakeBrowserCapability::default());
    let mut last = None;
    for _ in 0..MAX_SESSION_CONNECTIONS {
        let mut client = connect(&name);
        assert_eq!(send_hello(&mut client, &token).unwrap(), b"OK");
        assert_eq!(
            initialize(&mut client).unwrap()["result"]["serverInfo"]["name"],
            "sage-browser"
        );
        last = Some(client);
    }
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(150)
        )
        .is_err()
    );
    stop(live, worker);
    drop(last);
}

#[test]
fn wrong_token_does_not_evict_live_client() {
    let (name, token, live, worker) = start(FakeBrowserCapability::default());
    let mut original = connect(&name);
    assert_eq!(send_hello(&mut original, &token).unwrap(), b"OK");
    let mut intruder = connect(&name);
    let mut wrong = token.clone().into_bytes();
    wrong[0] = if wrong[0] == b'a' { b'b' } else { b'a' };
    assert!(send_hello(&mut intruder, &String::from_utf8(wrong).unwrap()).is_err());
    assert_eq!(
        initialize(&mut original).unwrap()["result"]["serverInfo"]["name"],
        "sage-browser"
    );
    let mut replacement = connect(&name);
    assert_eq!(send_hello(&mut replacement, &token).unwrap(), b"OK");
    assert_eq!(
        initialize(&mut replacement).unwrap()["result"]["serverInfo"]["name"],
        "sage-browser"
    );
    drop(original);
    drop(intruder);
    drop(replacement);
    stop(live, worker);
}

#[test]
fn failed_authentication_cap_preserves_current_client_and_closes_listener() {
    let (name, token, live, worker) = start(FakeBrowserCapability::default());
    let mut original = connect(&name);
    assert_eq!(send_hello(&mut original, &token).unwrap(), b"OK");
    let mut wrong = token.clone().into_bytes();
    wrong[0] = if wrong[0] == b'a' { b'b' } else { b'a' };
    for _ in 0..MAX_AUTH_FAILURES {
        let mut intruder = connect(&name);
        assert!(send_hello(&mut intruder, &String::from_utf8(wrong.clone()).unwrap()).is_err());
    }
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(150)
        )
        .is_err()
    );
    assert_eq!(
        initialize(&mut original).unwrap()["result"]["serverInfo"]["name"],
        "sage-browser"
    );
    stop(live, worker);
}

#[test]
fn ended_session_rejects_reconnection() {
    let (name, token, live, worker) = start(FakeBrowserCapability::default());
    let mut client = connect(&name);
    assert_eq!(send_hello(&mut client, &token).unwrap(), b"OK");
    stop(live, worker);
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(150)
        )
        .is_err()
    );
}

struct BlockingPort {
    fake: FakeBrowserCapability,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
}

impl BrowserCapability for BlockingPort {
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(5)).unwrap();
        self.fake.perform(request)
    }
}

#[test]
fn superseded_inflight_operation_does_not_reply_or_dispatch_twice() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut fake = FakeBrowserCapability::default();
    fake.open_tab(BrowserScope {
        session_id: "session-reconnect".into(),
        tab_id: 7,
    });
    let port = BlockingPort {
        fake,
        entered: entered_tx,
        release: release_rx,
        calls: Arc::clone(&calls),
    };
    let (name, token, live, worker) = start(port);
    let mut old = connect(&name);
    assert_eq!(send_hello(&mut old, &token).unwrap(), b"OK");
    old.write(
        &serde_json::to_vec(&json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"snapshotPage","arguments":{"scope":{"tabId":7}}}
        }))
        .unwrap(),
    )
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut replacement = connect(&name);
    replacement
        .write(
            &serde_json::to_vec(&json!({
                "session_id":"session-reconnect","token":token
            }))
            .unwrap(),
        )
        .unwrap();
    release_tx.send(()).unwrap();
    assert_eq!(read_frame(&mut replacement).unwrap(), b"OK");
    assert!(read_frame(&mut old).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        initialize(&mut replacement).unwrap()["result"]["serverInfo"]["name"],
        "sage-browser"
    );
    drop(old);
    drop(replacement);
    stop(live, worker);
}
