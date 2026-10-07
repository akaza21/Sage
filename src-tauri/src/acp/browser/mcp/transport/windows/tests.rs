use super::*;
use crate::acp::browser::{BrowserScope, FakeBrowserCapability};
use serde_json::json;

#[test]
fn name_and_owner_acl_are_bounded() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    validate_name(&host.name).unwrap();
    assert!(validate_name(r"\\.\pipe\other").is_err());
    PipeSecurity::new(&current_user_sid().unwrap()).unwrap();
}

#[test]
fn client_rejects_wrong_server_pid() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let waiter = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        accept(&host.pipe, &|| {
            if Instant::now() < deadline {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    assert!(
        connect_client(
            &name,
            std::process::id().wrapping_add(1),
            Instant::now() + Duration::from_secs(5)
        )
        .is_err()
    );
    // The rejected client can close before the host accepts it.
    let _ = waiter.join().unwrap();
}

#[test]
fn helper_can_detect_host_pipe_close_while_stdin_is_idle() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let client = connect_client(
        &host.name,
        std::process::id(),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    assert!(pipe_alive(&client.0));
    drop(host);
    assert!(!pipe_alive(&client.0));
}

#[test]
fn named_pipe_relay_serves_a_real_stdio_frame() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    };
    let mut port = FakeBrowserCapability::default();
    port.open_tab(scope.clone());
    let (mut server, token) = BrowserMcpServer::issue(port, scope, super::super::now_ms().unwrap());
    let waiter = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || BrowserMcpSessionState::Live)
    });
    let mut client = connect_client(
        &name,
        std::process::id(),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(100)
        )
        .is_err()
    );
    let mut input =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .unwrap();
    input.push(b'\n');
    let mut output = Vec::new();
    relay_stdio(
        &mut client,
        &mut input.as_slice(),
        &mut output,
        "session-1",
        token,
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&output).contains("snapshotPage"));
    drop(client);
    waiter.join().unwrap().unwrap();
    assert!(
        connect_client(
            &name,
            std::process::id(),
            Instant::now() + Duration::from_millis(100)
        )
        .is_err(),
        "a helper restart cannot silently reuse the consumed session pipe"
    );
}

#[test]
fn late_helper_connects_while_session_remains_live() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let issued_at = super::super::now_ms().unwrap().saturating_sub(61_000);
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        issued_at,
    );
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let worker_live = live.clone();
    let waiter = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || {
            if worker_live.load(std::sync::atomic::Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    let mut client = connect_client(
        &name,
        std::process::id(),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    let mut input =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .unwrap();
    input.push(b'\n');
    let mut output = Vec::new();
    relay_stdio(
        &mut client,
        &mut input.as_slice(),
        &mut output,
        "session-1",
        token,
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&output).contains("snapshotPage"));
    drop(client);
    live.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(waiter.join().unwrap(), Err(BrowserMcpError::Unavailable));
}

#[test]
fn listener_closes_when_session_ends_before_helper_connects() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let worker_live = live.clone();
    let (mut server, _) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        super::super::now_ms().unwrap(),
    );
    let waiter = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || {
            if worker_live.load(std::sync::atomic::Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Ended
            }
        })
    });
    live.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(waiter.join().unwrap(), Err(BrowserMcpError::Unavailable));
}

#[test]
fn listener_started_before_activation_accepts_after_epoch_becomes_live() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let name = host.name.clone();
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_live = live.clone();
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        super::super::now_ms().unwrap(),
    );
    let waiter = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || {
            if worker_live.load(std::sync::atomic::Ordering::SeqCst) {
                BrowserMcpSessionState::Live
            } else {
                BrowserMcpSessionState::Pending
            }
        })
    });
    std::thread::sleep(Duration::from_millis(20));
    live.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut client = connect_client(
        &name,
        std::process::id(),
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    let mut input =
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}))
            .unwrap();
    input.push(b'\n');
    let mut output = Vec::new();
    relay_stdio(
        &mut client,
        &mut input.as_slice(),
        &mut output,
        "session-1",
        token,
    )
    .unwrap();
    assert!(String::from_utf8_lossy(&output).contains("snapshotPage"));
    drop(client);
    live.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(waiter.join().unwrap(), Err(BrowserMcpError::Unavailable));
}

#[test]
fn failed_activation_closes_pending_listener() {
    let host = BrowserMcpPipeHost::bind().unwrap();
    let ended = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_ended = ended.clone();
    let (mut server, _) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        super::super::now_ms().unwrap(),
    );
    let waiter = std::thread::spawn(move || {
        host.serve(&mut server, "session-1", || {
            if worker_ended.load(std::sync::atomic::Ordering::SeqCst) {
                BrowserMcpSessionState::Ended
            } else {
                BrowserMcpSessionState::Pending
            }
        })
    });
    std::thread::sleep(Duration::from_millis(20));
    ended.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(waiter.join().unwrap(), Err(BrowserMcpError::Unavailable));
}
