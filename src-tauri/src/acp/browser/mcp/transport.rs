//! Single-session MCP stdio bridge. Production admission remains deliberately separate.
use super::{BrowserMcpError, BrowserMcpServer, BrowserMcpToken, read_bounded_line};
use crate::acp::browser::BrowserCapability;
use sage_core::limits::BROWSER_LIMITS;
use std::io::{BufRead, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HELPER_ARGUMENT: &str = "--sage-browser-mcp-stdio";
const NO_REPLY: &[u8] = b"\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BrowserMcpSessionState {
    Pending,
    Live,
    Ended,
}

fn now_ms() -> Result<u64, BrowserMcpError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BrowserMcpError::Unavailable)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| BrowserMcpError::Unavailable)
}

/// The pipe side is message-framed. A notification receives an internal sentinel,
/// which the stdio side never forwards to the MCP client.
pub trait MessagePipe {
    /// `None` means no message is ready; callers must recheck cancellation.
    fn read(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, BrowserMcpError>;
    fn write(&mut self, bytes: &[u8]) -> Result<(), BrowserMcpError>;
}

pub fn serve_authenticated<P: BrowserCapability>(
    server: &mut BrowserMcpServer<P>,
    pipe: &mut impl MessagePipe,
    session_id: &str,
    session_live: impl Fn() -> bool,
) -> Result<(), BrowserMcpError> {
    if !session_live() {
        return Err(BrowserMcpError::Unavailable);
    }
    let handshake = loop {
        if !session_live() {
            return Err(BrowserMcpError::Unavailable);
        }
        if let Some(frame) = pipe.read(64 + BROWSER_LIMITS.session_id_bytes + 128)? {
            break frame;
        }
    };
    let hello: Handshake =
        serde_json::from_slice(&handshake).map_err(|_| BrowserMcpError::Unauthorized)?;
    if hello.session_id != session_id || !session_live() {
        return Err(BrowserMcpError::Unauthorized);
    }
    let token = BrowserMcpToken::from_env_value(hello.token)?;
    server.connect(&token, &hello.session_id, now_ms()?)?;
    pipe.write(b"OK")?;
    loop {
        if !session_live() {
            return Err(BrowserMcpError::Unavailable);
        }
        let frame = match pipe.read(BROWSER_LIMITS.request_bytes) {
            Ok(Some(frame)) => frame,
            Ok(None) => continue,
            Err(BrowserMcpError::Unavailable) if session_live() => return Ok(()),
            Err(error) => return Err(error),
        };
        if !session_live() {
            return Err(BrowserMcpError::Unavailable);
        }
        let response = server.handle(&frame)?;
        pipe.write(if response.is_empty() {
            NO_REPLY
        } else {
            &response
        })?;
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Handshake {
    session_id: String,
    token: String,
}

pub fn relay_stdio<R: BufRead, W: Write>(
    pipe: &mut impl MessagePipe,
    reader: &mut R,
    writer: &mut W,
    session_id: &str,
    token: BrowserMcpToken,
) -> Result<(), BrowserMcpError> {
    let hello = serde_json::to_vec(&Handshake {
        session_id: session_id.to_owned(),
        token: token.into_env_value(),
    })
    .map_err(|_| BrowserMcpError::Unavailable)?;
    pipe.write(&hello)?;
    if read_required(pipe, 2)? != b"OK" {
        return Err(BrowserMcpError::Unauthorized);
    }
    let mut frame = Vec::new();
    while read_bounded_line(reader, &mut frame)? {
        pipe.write(&frame)?;
        let reply = read_required(pipe, BROWSER_LIMITS.result_bytes)?;
        if reply != NO_REPLY {
            writer
                .write_all(&reply)
                .and_then(|_| writer.write_all(b"\n"))
                .and_then(|_| writer.flush())
                .map_err(|_| BrowserMcpError::Unavailable)?;
        }
        frame.clear();
    }
    Ok(())
}

fn read_required(pipe: &mut impl MessagePipe, maximum: usize) -> Result<Vec<u8>, BrowserMcpError> {
    loop {
        if let Some(frame) = pipe.read(maximum)? {
            return Ok(frame);
        }
    }
}

#[cfg(windows)]
mod windows;
#[cfg(all(windows, test, feature = "acp-fixture", debug_assertions))]
pub(crate) use windows::connect_for_test;
#[cfg(windows)]
pub use windows::{BrowserMcpPipeHost, run_stdio_helper_if_requested};

#[cfg(test)]
mod tests;
