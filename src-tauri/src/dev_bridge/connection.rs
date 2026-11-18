use super::{
    protocol::{OUTBOUND_FRAMES, Outbound, Request},
    transport::Session,
};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{
    net::TcpStream,
    sync::{mpsc, watch},
    time::Instant,
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub(super) enum SessionEvent {
    Request(Request, usize),
    CloseRequested,
}

pub(super) async fn run(
    socket: WebSocketStream<TcpStream>,
    session: Session,
    handle: impl FnMut(SessionEvent, &Session, &Outbound),
) {
    run_with_timeout(socket, session, handle, Duration::from_secs(30)).await;
}

async fn run_with_timeout(
    socket: WebSocketStream<TcpStream>,
    mut session: Session,
    mut handle: impl FnMut(SessionEvent, &Session, &Outbound),
    pong_timeout: Duration,
) {
    let (sender, mut receiver) = mpsc::channel(OUTBOUND_FRAMES);
    let (stop, mut cancelled) = watch::channel(false);
    let output = Outbound::new(sender, stop);
    let (mut writer, mut reader) = socket.split();
    let mut deadline = Instant::now() + pong_timeout;
    let mut awaiting_pong = false;
    loop {
        let frame = tokio::select! {
            _ = cancelled.changed() => break,
            _ = tokio::time::sleep_until(deadline) => {
                if awaiting_pong { break; }
                awaiting_pong = true;
                deadline = Instant::now() + pong_timeout;
                Message::Ping(b"sage".to_vec().into())
            }
            _ = session.close_requested() => {
                handle(SessionEvent::CloseRequested, &session, &output);
                continue;
            }
            frame = receiver.recv() => {
                let Some(frame) = frame else { break; };
                output.dequeued(&frame);
                frame
            }
            incoming = reader.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(request) = serde_json::from_str::<Request>(&text) else { break; };
                        session.observe_request(&request);
                        handle(SessionEvent::Request(request, text.len()), &session, &output);
                        continue;
                    }
                    Some(Ok(Message::Pong(payload))) => {
                        if awaiting_pong && payload.as_ref() == b"sage" {
                            awaiting_pong = false;
                            deadline = Instant::now() + pong_timeout;
                        }
                        continue;
                    }
                    Some(Ok(Message::Ping(payload))) => Message::Pong(payload),
                    _ => break,
                }
            }
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(5), writer.send(frame)).await,
            Ok(Ok(()))
        ) {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
