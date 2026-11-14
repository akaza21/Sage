use super::*;
use crate::acp::browser::mcp::MAX_SESSION_CONNECTIONS;
use sage_core::limits::BROWSER_LIMITS;

const MAX_AUTH_FAILURES: u8 = 4;
const HELLO_TIMEOUT: Duration = Duration::from_secs(10);

struct Listener {
    pipe: Pipe,
    connected_since: Option<Instant>,
}

struct ConnectionBudget {
    accepted: u8,
    failed_auth: u8,
}

#[derive(Debug)]
enum ReconnectDisabledReason {
    ListenerUnavailable,
}

pub(super) fn serve<P: BrowserCapability>(
    host: BrowserMcpPipeHost,
    server: &mut BrowserMcpServer<P>,
    session_id: &str,
    session_state: impl Fn() -> BrowserMcpSessionState,
) -> Result<(), BrowserMcpError> {
    let started = Instant::now();
    if let Err(error) = accept(&host.pipe, &session_state) {
        tracing::warn!(
            phase = "never_connected",
            elapsed_ms = started.elapsed().as_millis() as u64,
            status = ?error,
            "Sage browser MCP pipe ended"
        );
        return Err(error);
    }
    let result = serve_connected(host, server, session_id, &session_state, &new_listener);
    tracing::warn!(
        phase = "connected",
        elapsed_ms = started.elapsed().as_millis() as u64,
        status = ?result.as_ref().err(),
        "Sage browser MCP pipe ended"
    );
    result
}

fn serve_connected<P: BrowserCapability>(
    host: BrowserMcpPipeHost,
    server: &mut BrowserMcpServer<P>,
    session_id: &str,
    session_state: &impl Fn() -> BrowserMcpSessionState,
    listener_factory: &impl Fn(&str) -> Result<Listener, BrowserMcpError>,
) -> Result<(), BrowserMcpError> {
    let mut active = Some(host.pipe);
    let first = active.as_mut().ok_or(BrowserMcpError::Unavailable)?;
    let first_hello_deadline = Instant::now() + HELLO_TIMEOUT;
    let hello = loop {
        ensure_live(session_state)?;
        if Instant::now() >= first_hello_deadline {
            return Err(BrowserMcpError::Unavailable);
        }
        if let Some(hello) = first.read(handshake_limit())? {
            break hello;
        }
    };
    authenticate(server, session_id, &hello, false, session_state)?;
    first.write(b"OK")?;
    let mut budget = ConnectionBudget {
        accepted: 1,
        failed_auth: 0,
    };
    let mut listener = if server.supports_reconnect() {
        install_listener(&host.name, listener_factory)
    } else {
        None
    };
    loop {
        ensure_live(session_state)?;
        let _ = poll_replacement(
            (&host.name, listener_factory),
            &mut active,
            &mut listener,
            server,
            session_id,
            session_state,
            &mut budget,
        )?;
        let Some(pipe) = active.as_mut() else {
            if listener.is_none() {
                return Err(BrowserMcpError::Unavailable);
            }
            std::thread::sleep(Duration::from_millis(25));
            continue;
        };
        let frame = match pipe.read(BROWSER_LIMITS.request_bytes) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(BrowserMcpError::Unavailable) => {
                if listener.is_none() {
                    return Ok(());
                }
                active = None;
                continue;
            }
            Err(error) => return Err(error),
        };
        ensure_live(session_state)?;
        let response = server.handle(&frame)?;
        ensure_live(session_state)?;
        let replaced = poll_replacement(
            (&host.name, listener_factory),
            &mut active,
            &mut listener,
            server,
            session_id,
            session_state,
            &mut budget,
        )?;
        if replaced || active.is_none() {
            continue;
        }
        let pipe = active.as_mut().ok_or(BrowserMcpError::Unavailable)?;
        if pipe
            .write(if response.is_empty() {
                super::super::NO_REPLY
            } else {
                &response
            })
            .is_err()
        {
            active = None;
        }
    }
}

fn poll_replacement<P: BrowserCapability>(
    source: (&str, &impl Fn(&str) -> Result<Listener, BrowserMcpError>),
    active: &mut Option<Pipe>,
    listener: &mut Option<Listener>,
    server: &mut BrowserMcpServer<P>,
    session_id: &str,
    session_state: &impl Fn() -> BrowserMcpSessionState,
    budget: &mut ConnectionBudget,
) -> Result<bool, BrowserMcpError> {
    let (name, listener_factory) = source;
    let Some(candidate) = listener.as_mut() else {
        return Ok(false);
    };
    if candidate.connected_since.is_none() {
        if unsafe {
            ConnectNamedPipe(
                candidate.pipe.0.as_raw_handle().cast(),
                std::ptr::null_mut(),
            )
        } != 0
        {
            candidate.connected_since = Some(Instant::now());
        } else {
            match unsafe { GetLastError() } {
                ERROR_PIPE_CONNECTED => candidate.connected_since = Some(Instant::now()),
                ERROR_PIPE_LISTENING | ERROR_NO_DATA => return Ok(false),
                _ => return Err(BrowserMcpError::Unavailable),
            }
        }
    }
    let hello = match candidate.pipe.read(handshake_limit()) {
        Ok(Some(hello)) => hello,
        Ok(None)
            if candidate
                .connected_since
                .is_some_and(|since| since.elapsed() < HELLO_TIMEOUT) =>
        {
            return Ok(false);
        }
        Err(_) => {
            *listener = None;
            budget.failed_auth += 1;
            if budget.failed_auth < MAX_AUTH_FAILURES {
                *listener = install_listener(name, listener_factory);
            }
            return Ok(false);
        }
        Ok(None) => {
            *listener = None;
            budget.failed_auth += 1;
            if budget.failed_auth < MAX_AUTH_FAILURES {
                *listener = install_listener(name, listener_factory);
            }
            return Ok(false);
        }
    };
    ensure_live(session_state)?;
    if authenticate(server, session_id, &hello, true, session_state).is_err() {
        *listener = None;
        budget.failed_auth += 1;
        if budget.failed_auth < MAX_AUTH_FAILURES {
            *listener = install_listener(name, listener_factory);
        }
        return Ok(false);
    }
    let replacement = listener.take().ok_or(BrowserMcpError::Unavailable)?.pipe;
    *active = Some(replacement);
    budget.accepted += 1;
    ensure_live(session_state)?;
    if active
        .as_mut()
        .ok_or(BrowserMcpError::Unavailable)?
        .write(b"OK")
        .is_err()
    {
        *active = None;
    }
    if budget.accepted < MAX_SESSION_CONNECTIONS {
        *listener = install_listener(name, listener_factory);
    }
    Ok(true)
}

fn install_listener(
    name: &str,
    listener_factory: &impl Fn(&str) -> Result<Listener, BrowserMcpError>,
) -> Option<Listener> {
    match listener_factory(name) {
        Ok(listener) => Some(listener),
        Err(_) => {
            tracing::warn!(
                reason = ?ReconnectDisabledReason::ListenerUnavailable,
                "Sage browser MCP reconnect disabled"
            );
            None
        }
    }
}

fn authenticate<P: BrowserCapability>(
    server: &mut BrowserMcpServer<P>,
    session_id: &str,
    bytes: &[u8],
    reconnect: bool,
    session_state: &impl Fn() -> BrowserMcpSessionState,
) -> Result<(), BrowserMcpError> {
    let hello: super::super::Handshake =
        serde_json::from_slice(bytes).map_err(|_| BrowserMcpError::Unauthorized)?;
    if hello.session_id != session_id {
        return Err(BrowserMcpError::Unauthorized);
    }
    ensure_live(session_state)?;
    let token = BrowserMcpToken::from_env_value(hello.token)?;
    if reconnect {
        server.reconnect(&token, &hello.session_id)?;
    } else {
        server.connect(&token, &hello.session_id, super::super::now_ms()?)?;
    }
    Ok(())
}

fn ensure_live(session_state: &impl Fn() -> BrowserMcpSessionState) -> Result<(), BrowserMcpError> {
    (session_state() == BrowserMcpSessionState::Live)
        .then_some(())
        .ok_or(BrowserMcpError::Unavailable)
}

fn handshake_limit() -> usize {
    64 + BROWSER_LIMITS.session_id_bytes + 128
}

fn new_listener(name: &str) -> Result<Listener, BrowserMcpError> {
    let security = PipeSecurity::new(&current_user_sid()?)?;
    let wide = wide(name);
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            2,
            PIPE_BYTES,
            PIPE_BYTES,
            50,
            &security.attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(BrowserMcpError::Unavailable);
    }
    Ok(Listener {
        pipe: Pipe(unsafe { File::from_raw_handle(handle.cast()) }),
        connected_since: None,
    })
}

#[cfg(test)]
mod tests;
