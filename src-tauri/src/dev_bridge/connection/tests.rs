use super::*;
use crate::dev_bridge::{
    auth::Pairing,
    transport::{self, Access},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    client_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue},
};

async fn connect(listener: &TcpListener, credentials: Value) -> WebSocketStream<TcpStream> {
    let address = listener.local_addr().unwrap();
    let mut request = format!("ws://{address}/").into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Origin", HeaderValue::from_static("http://127.0.0.1:23456"));
    let stream = TcpStream::connect(address).await.unwrap();
    let (mut socket, _) = client_async(request, stream).await.unwrap();
    socket
        .send(Message::Text(credentials.to_string().into()))
        .await
        .unwrap();
    let reply = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(serde_json::from_str::<Value>(&reply).unwrap()["session"].is_string());
    socket
}

async fn fixture(
    pong_timeout: Duration,
) -> (
    WebSocketStream<TcpStream>,
    tokio::task::JoinHandle<()>,
    Arc<Access>,
) {
    let listener = Arc::new(TcpListener::bind("127.0.0.1:0").await.unwrap());
    let access = Arc::new(Access::new(
        listener.local_addr().unwrap().to_string(),
        "http://127.0.0.1:23456".into(),
        Arc::new(Mutex::new(Pairing::new("a".repeat(64)))),
    ));
    let server_access = access.clone();
    let server_listener = listener.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = server_listener.accept().await.unwrap();
        let (socket, session) = transport::accept(stream, &server_access).await.unwrap();
        run_with_timeout(
            socket,
            session,
            |event, session, output| match event {
                SessionEvent::Request(request, _) => {
                    output.send(json!({"id":request.id,"ok":request.args}))
                }
                SessionEvent::CloseRequested => {
                    if session.forward_close_request() {
                        output.event("tauri://close-requested", 1, "null");
                    }
                }
            },
            pong_timeout,
        )
        .await;
    });
    let socket = connect(&listener, json!({"pairingCode":"a".repeat(64)})).await;
    (socket, server, access)
}

#[tokio::test]
async fn missing_pong_releases_the_active_session() {
    let (mut browser, server, access) = fixture(Duration::from_millis(100)).await;
    let ping = browser.next().await.unwrap().unwrap();
    assert!(matches!(ping, Message::Ping(_)));
    // Do not poll or flush: tungstenite has queued its automatic pong but cannot send it.
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !access.request_close(),
        "the expired session must release its lease"
    );
}

#[tokio::test]
async fn matching_pongs_keep_the_connection_usable_across_deadlines() {
    let (mut browser, server, _) = fixture(Duration::from_secs(1)).await;
    for id in 0..3 {
        assert!(matches!(
            browser.next().await.unwrap().unwrap(),
            Message::Ping(_)
        ));
        browser.flush().await.unwrap();
        browser
            .send(Message::Text(
                json!({"id":id,"cmd":"get_bootstrap_snapshot","args":{"alive":id}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let reply = browser.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&reply).unwrap(),
            json!({"id":id,"ok":{"alive":id}})
        );
    }
    browser.close(None).await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn close_answer_on_the_wire_rearms_native_forwarding() {
    let (mut browser, server, access) = fixture(Duration::from_secs(30)).await;
    for (id, status) in [(1, "error"), (2, "end")] {
        assert!(access.request_close());
        let event = browser.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&event).unwrap()["event"],
            "tauri://close-requested"
        );
        assert!(
            !access.request_close(),
            "unanswered closes retain the escape hatch"
        );
        browser.send(Message::Text(json!({"id":id,"cmd":"trace_close_stage","args":{"stage":"closeRequested","status":status}}).to_string().into())).await.unwrap();
        let reply = browser.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&reply).unwrap()["ok"]["status"],
            status
        );
    }
    browser.close(None).await.unwrap();
    server.await.unwrap();
}
