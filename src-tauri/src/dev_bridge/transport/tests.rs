use super::*;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    client_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue},
};

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

async fn response(socket: &mut WebSocketStream<TcpStream>) -> serde_json::Value {
    let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(&message.into_text().unwrap()).unwrap()
}

#[tokio::test]
async fn websocket_handshake_rejects_foreign_host_and_origin_and_consumed_or_busy_pairing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:23456";
    let access = Arc::new(Access::new(
        address.to_string(),
        origin.into(),
        Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    ));
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

#[tokio::test]
async fn reconnect_preserves_admission_until_dispatched_operations_respond() {
    use super::super::protocol::{MAX_PENDING, Outbound, PendingInvoke, Request};
    use tauri::ipc::{InvokeResponse, InvokeResponseBody};
    use tokio::sync::{mpsc, watch};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:23456";
    let access = Access::new(
        address.to_string(),
        origin.into(),
        Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    );
    let (dispatched, mut callbacks) = mpsc::channel::<PendingInvoke>(MAX_PENDING);
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut socket, session) = accept(stream, &access).await.unwrap();
            let (sender, mut receiver) = mpsc::channel(MAX_PENDING);
            let (stop, _cancelled) = watch::channel(false);
            let output = Outbound::new(sender, stop);
            loop {
                tokio::select! {
                    incoming = socket.next() => {
                        match incoming.unwrap().unwrap() {
                            Message::Text(text) => {
                                let request: Request = serde_json::from_str(&text).unwrap();
                                if let Some(reply) = session.admit(request.id, &output) {
                                    dispatched.send(reply).await.unwrap();
                                }
                            }
                            Message::Close(_) => break,
                            _ => panic!("unexpected client frame"),
                        }
                    }
                    frame = receiver.recv() => {
                        let frame = frame.unwrap();
                        output.dequeued(&frame);
                        socket.send(frame).await.unwrap();
                    }
                }
            }
            drop(session);
            let _ = socket.close(None).await;
        }
    });
    let mut first = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    let session = pair(
        &mut first,
        serde_json::json!({"pairingCode":"a".repeat(64)}),
    )
    .await
    .unwrap();
    let mut outstanding = Vec::new();
    for id in 0..MAX_PENDING {
        first
            .send(Message::Text(
                serde_json::json!({"id":id,"cmd":"pick_project_source_folder"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        outstanding.push(
            tokio::time::timeout(Duration::from_secs(5), callbacks.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    first.close(None).await.unwrap();
    let _ = first.next().await;
    let mut reload = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    pair(&mut reload, session).await.unwrap();
    reload
        .send(Message::Text(
            r#"{"id":100,"cmd":"pick_project_source_folder"}"#.into(),
        ))
        .await
        .unwrap();
    assert_eq!(
        response(&mut reload).await,
        serde_json::json!({"id":100,"err":"bridge invoke limit reached"})
    );
    outstanding
        .pop()
        .unwrap()
        .respond(InvokeResponse::Ok(InvokeResponseBody::Json("null".into())));
    reload
        .send(Message::Text(
            r#"{"id":101,"cmd":"pick_project_source_folder"}"#.into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), callbacks.recv())
        .await
        .unwrap()
        .unwrap()
        .respond(InvokeResponse::Ok(InvokeResponseBody::Json(
            "\"selected-folder\"".into(),
        )));
    assert_eq!(
        response(&mut reload).await,
        serde_json::json!({"id":101,"ok":"selected-folder"})
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn websocket_accepts_maximum_image_and_exact_bound_but_rejects_one_byte_over() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use tokio_tungstenite::tungstenite::Error;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:23456";
    let access = Access::new(
        address.to_string(),
        origin.into(),
        Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    );
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut socket, _lease) = accept(stream, &access).await.unwrap();
        let image = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let upload: super::super::protocol::Request = serde_json::from_str(&image).unwrap();
        assert_eq!(upload.cmd, "upload_chat_image");
        assert_eq!(
            STANDARD
                .decode(upload.args["request"]["dataBase64"].as_str().unwrap())
                .unwrap()
                .len(),
            sage_core::limits::IMAGE_UPLOAD_BYTES
        );
        drop(upload);
        drop(image);
        assert_eq!(
            socket.next().await.unwrap().unwrap().len(),
            MAX_MESSAGE_BYTES
        );
        assert!(matches!(socket.next().await, Some(Err(Error::Capacity(_)))));
    });
    let mut socket = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    pair(
        &mut socket,
        serde_json::json!({"pairingCode":"a".repeat(64)}),
    )
    .await
    .unwrap();
    let image = serde_json::json!({
        "id": u32::MAX, "cmd": "upload_chat_image", "args": {"request": {
            "projectId": "\0".repeat(512), "projectGeneration": u64::MAX,
            "chatId": "\0".repeat(512), "mediaType": "image/jpeg",
            "dataBase64": STANDARD.encode(vec![0; sage_core::limits::IMAGE_UPLOAD_BYTES]),
            "label": "\0".repeat(80)
        }}
    });
    socket
        .send(Message::Text(image.to_string().into()))
        .await
        .unwrap();
    drop(image);
    socket
        .send(Message::Text("a".repeat(MAX_MESSAGE_BYTES).into()))
        .await
        .unwrap();
    let _ = socket
        .send(Message::Text("a".repeat(MAX_MESSAGE_BYTES + 1).into()))
        .await;
    server.await.unwrap();
}

#[tokio::test]
async fn native_close_waits_once_for_the_browser_and_reconnect_discards_stale_requests() {
    let listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
    let address = listener.local_addr().unwrap();
    let origin = "http://127.0.0.1:23456";
    let access = Arc::new(Access::new(
        address.to_string(),
        origin.into(),
        Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    ));
    assert!(!access.request_close());
    let server_access = access.clone();
    let server_listener = listener.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = server_listener.accept().await.unwrap();
        accept(stream, &server_access).await.unwrap()
    });
    let mut browser = connect(address, &address.to_string(), origin)
        .await
        .unwrap();
    let credentials = pair(
        &mut browser,
        serde_json::json!({"pairingCode":"a".repeat(64)}),
    )
    .await
    .unwrap();
    let (_socket, session) = server.await.unwrap();
    assert!(access.request_close());
    assert!(
        !access.request_close(),
        "a browser without a listener cannot trap close"
    );
    assert!(session.forward_close_request());
    assert!(!session.forward_close_request());
    assert!(
        !access.request_close(),
        "a hung close handler cannot trap close"
    );
    drop(session);
    assert!(!access.request_close());

    // Exercise reconnect with both an undelivered and a forwarded request.
    for forwarded in [false, true, false] {
        let (accepted, connected) = tokio::join!(
            async {
                let (stream, _) = listener.accept().await.unwrap();
                accept(stream, &access).await.unwrap()
            },
            async {
                let mut browser = connect(address, &address.to_string(), origin)
                    .await
                    .unwrap();
                pair(&mut browser, credentials.clone()).await.unwrap();
                browser
            }
        );
        let (_socket, session) = accepted;
        let _browser = connected;
        assert!(
            !session.forward_close_request(),
            "reconnect must not replay a stale close"
        );
        assert!(
            access.request_close(),
            "reconnect starts a fresh close handshake"
        );
        if forwarded {
            assert!(session.forward_close_request());
        }
        assert!(!access.request_close());
        drop(session);
        // Leave the latch untouched until the next authentication.
    }
    assert!(
        !access.request_close(),
        "a disconnected browser cannot trap close"
    );
}
