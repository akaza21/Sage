use super::{
    auth::{Credentials, Lease, Pairing, lock},
    protocol::MAX_MESSAGE_BYTES,
};
use futures_util::{SinkExt, StreamExt};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        protocol::WebSocketConfig,
    },
};

pub(super) struct Access {
    pub host: String,
    pub origin: String,
    pub pairing: Arc<Mutex<Pairing>>,
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
) -> Result<(WebSocketStream<TcpStream>, Lease), ()> {
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_BYTES))
        .max_frame_size(Some(MAX_MESSAGE_BYTES))
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_MESSAGE_BYTES + 1024);
    let handshake = async {
        let mut socket =
            accept_hdr_async_with_config(stream, UpgradeGuard { access }, Some(config))
                .await
                .map_err(|_| ())?;
        let Some(Ok(Message::Text(text))) = socket.next().await else {
            return Err(());
        };
        if text.len() > 256 {
            return Err(());
        }
        let credentials: Credentials = serde_json::from_str(&text).map_err(|_| ())?;
        let session = lock(&access.pairing)
            .authenticate(credentials)
            .map_err(|_| ())?;
        let lease = Lease(access.pairing.clone());
        let reply = serde_json::json!({"session":session}).to_string();
        socket
            .send(Message::Text(reply.into()))
            .await
            .map_err(|_| ())?;
        Ok((socket, lease))
    };
    tokio::time::timeout(Duration::from_secs(2), handshake)
        .await
        .map_err(|_| ())?
}

#[cfg(test)]
mod tests;
