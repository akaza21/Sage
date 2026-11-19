use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, BufReader, ReadBuf},
    net::TcpStream,
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::protocol::{Role, WebSocketConfig},
};

/// The async WebSocket adapter cannot change configuration after construction.
/// Keep read-ahead here until authentication completes, so promoting to a new
/// WebSocket codec preserves every byte without retaining the tiny auth limits.
pub(super) struct HandshakeIo {
    reader: BufReader<TcpStream>,
    authenticating: bool,
}

impl HandshakeIo {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            reader: BufReader::with_capacity(4096, stream),
            authenticating: false,
        }
    }

    pub fn begin_authentication(&mut self) {
        self.authenticating = true;
    }

    pub async fn promote(
        socket: WebSocketStream<Self>,
        config: WebSocketConfig,
    ) -> WebSocketStream<TcpStream> {
        let buffered = socket.into_inner().reader;
        let unread = buffered.buffer().to_vec();
        WebSocketStream::from_partially_read(
            buffered.into_inner(),
            unread,
            Role::Server,
            Some(config),
        )
        .await
    }
}

impl AsyncRead for HandshakeIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if output.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        // HTTP upgrade reads must stay batched: tungstenite treats repeated tiny
        // reads as an attack. Only credential frames need the codec read limit.
        if !self.authenticating {
            return Pin::new(&mut self.reader).poll_read(cx, output);
        }
        // A byte at a time into the codec prevents its private buffer from
        // consuming any of the next frame. BufReader batches the actual TCP IO.
        let mut byte = [0];
        let mut limited = ReadBuf::new(&mut byte);
        ready!(Pin::new(&mut self.reader).poll_read(cx, &mut limited))?;
        output.put_slice(limited.filled());
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for HandshakeIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.reader.get_mut()).poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.reader.get_mut()).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.reader.get_mut()).poll_shutdown(cx)
    }
}
