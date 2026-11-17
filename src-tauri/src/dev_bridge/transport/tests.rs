use super::*;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    client_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue},
};

#[tokio::test]
async fn websocket_handshake_rejects_foreign_host_and_origin_and_consumed_or_busy_pairing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:23456";
    let access = Arc::new(Access {
        host: address.to_string(),
        origin: origin.into(),
        pairing: Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    });
    let server = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let (stream, _) = accepted.unwrap();
                    let access = access.clone();
                    tasks.spawn(async move {
                        if let Ok((mut socket, _lease)) = accept(stream, &access).await {
                            while let Some(Ok(message)) = socket.next().await {
                                if message.is_close() { break; }
                            }
                        }
                    });
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
    });
    async fn connect(
        address: std::net::SocketAddr,
        host: &str,
        origin: &str,
    ) -> Result<WebSocketStream<TcpStream>, ()> {
        let mut request = format!("ws://{address}/").into_client_request().unwrap();
        request
            .headers_mut()
            .insert("Host", HeaderValue::from_str(host).unwrap());
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(origin).unwrap());
        let stream = TcpStream::connect(address).await.unwrap();
        client_async(request, stream)
            .await
            .map(|(socket, _)| socket)
            .map_err(|_| ())
    }
    async fn pair(
        socket: &mut WebSocketStream<TcpStream>,
        credentials: serde_json::Value,
    ) -> Option<serde_json::Value> {
        socket
            .send(Message::Text(credentials.to_string().into()))
            .await
            .unwrap();
        match socket.next().await {
            Some(Ok(Message::Text(text))) => Some(serde_json::from_str(&text).unwrap()),
            _ => None,
        }
    }
    assert!(
        connect(address, &address.to_string(), "http://evil.test")
            .await
            .is_err()
    );
    assert!(connect(address, "localhost:23456", origin).await.is_err());
    assert!(
        connect(address, &address.to_string(), &format!("{origin}/"))
            .await
            .is_err()
    );
    let mut wrong = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    assert!(
        pair(
            &mut wrong,
            serde_json::json!({"pairingCode":"c".repeat(64)})
        )
        .await
        .is_none()
    );
    let mut first = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    let session = pair(
        &mut first,
        serde_json::json!({"pairingCode":"a".repeat(64)}),
    )
    .await
    .unwrap();
    assert!(
        connect(address, &address.to_string(), origin)
            .await
            .is_err()
    );
    first.close(None).await.unwrap();
    let _ = first.next().await;
    let mut reuse = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    assert!(
        pair(
            &mut reuse,
            serde_json::json!({"pairingCode":"a".repeat(64)})
        )
        .await
        .is_none()
    );
    let mut reload = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    assert_eq!(pair(&mut reload, session.clone()).await.unwrap(), session);
    server.abort();
    let _ = server.await;
}
