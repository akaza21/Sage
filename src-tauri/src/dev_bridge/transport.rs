use super::{
    auth::{Credentials, Lease, Pairing, lock},
    handshake_io::HandshakeIo,
    protocol::{
        MAX_MESSAGE_BYTES, MAX_PENDING, MAX_PENDING_BYTES, Outbound, PendingInvoke,
        Request as BridgeRequest,
    },
};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{Semaphore, watch},
};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        protocol::WebSocketConfig,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseRequest {
    Idle,
    Pending,
    Forwarded,
}

pub(super) struct Access {
    host: String,
    origin: String,
    pairing: Arc<Mutex<Pairing>>,
    permits: Arc<Semaphore>,
    body_permits: Arc<Semaphore>,
    close_requests: watch::Sender<CloseRequest>,
}

impl Access {
    pub fn new(host: String, origin: String, pairing: Arc<Mutex<Pairing>>) -> Self {
        Self {
            host,
            origin,
            pairing,
            permits: Arc::new(Semaphore::new(MAX_PENDING)),
            body_permits: Arc::new(Semaphore::new(MAX_PENDING_BYTES)),
            close_requests: watch::channel(CloseRequest::Idle).0,
        }
    }

    pub fn request_close(&self) -> bool {
        let pairing = lock(&self.pairing);
        if !pairing.is_active() {
            self.close_requests.send_replace(CloseRequest::Idle);
            return false;
        }
        self.close_requests.send_if_modified(|state| {
            if *state != CloseRequest::Idle {
                return false;
            }
            *state = CloseRequest::Pending;
            true
        })
    }
}

pub(super) struct Session {
    _lease: Lease,
    permits: Arc<Semaphore>,
    body_permits: Arc<Semaphore>,
    close_requests: watch::Sender<CloseRequest>,
    close_changes: watch::Receiver<CloseRequest>,
}

impl Session {
    pub fn admit(&self, id: u32, body_bytes: usize, output: &Outbound) -> Option<PendingInvoke> {
        output.admit(id, body_bytes, &self.permits, &self.body_permits)
    }

    pub fn observe_request(&self, request: &BridgeRequest) {
        if request.cmd == "trace_close_stage"
            && request
                .args
                .get("stage")
                .and_then(serde_json::Value::as_str)
                == Some("closeRequested")
            && matches!(
                request
                    .args
                    .get("status")
                    .and_then(serde_json::Value::as_str),
                Some("error" | "end")
            )
        {
            self.close_requests.send_if_modified(|state| {
                if *state != CloseRequest::Forwarded {
                    return false;
                }
                *state = CloseRequest::Idle;
                true
            });
        }
    }

    pub async fn close_requested(&mut self) -> Result<(), watch::error::RecvError> {
        self.close_changes.changed().await
    }

    pub fn forward_close_request(&self) -> bool {
        self.close_requests.send_if_modified(|state| {
            if *state != CloseRequest::Pending {
                return false;
            }
            // Forwarding must retain the native second-close escape hatch.
            *state = CloseRequest::Forwarded;
            true
        })
    }
}

struct UpgradeGuard<'a> {
    access: &'a Access,
}

impl Callback for UpgradeGuard<'_> {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let exact_header = |name: &str, expected: &str| {
            request.headers().get_all(name).iter().count() == 1
                && request
                    .headers()
                    .get(name)
                    .and_then(|value| value.to_str().ok())
                    == Some(expected)
        };
        if !exact_header("host", &self.access.host)
            || !exact_header("origin", &self.access.origin)
            || lock(&self.access.pairing).is_active()
        {
            return Err(Response::builder()
                .status(403)
                .body(Some("bridge refused".into()))
                .unwrap());
        }
        Ok(response)
    }
}

pub(super) async fn accept(
    stream: TcpStream,
    access: &Access,
) -> Result<(WebSocketStream<TcpStream>, Session), ()> {
    let config = socket_config(256).read_buffer_size(256);
    let handshake = async {
        let mut socket = accept_hdr_async_with_config(
            HandshakeIo::new(stream),
            UpgradeGuard { access },
            Some(config),
        )
        .await
        .map_err(|_| ())?;
        socket.get_mut().begin_authentication();
        let Some(Ok(Message::Text(text))) = socket.next().await else {
            return Err(());
        };
        let credentials: Credentials = serde_json::from_str(&text).map_err(|_| ())?;
        let session = {
            let mut pairing = lock(&access.pairing);
            let session = pairing.authenticate(credentials).map_err(|_| ())?;
            // Serialize reset with native close admission so a new request is never lost.
            access.close_requests.send_replace(CloseRequest::Idle);
            session
        };
        let client = Session {
            _lease: Lease(access.pairing.clone()),
            permits: access.permits.clone(),
            body_permits: access.body_permits.clone(),
            close_requests: access.close_requests.clone(),
            close_changes: access.close_requests.subscribe(),
        };
        let reply = serde_json::json!({"session":session}).to_string();
        socket
            .send(Message::Text(reply.into()))
            .await
            .map_err(|_| ())?;
        let socket = HandshakeIo::promote(socket, socket_config(MAX_MESSAGE_BYTES)).await;
        Ok((socket, client))
    };
    tokio::time::timeout(Duration::from_secs(2), handshake)
        .await
        .map_err(|_| ())?
}

fn socket_config(message_bytes: usize) -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(message_bytes))
        .max_frame_size(Some(message_bytes))
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_MESSAGE_BYTES + 1024)
}

#[cfg(test)]
mod tests;
