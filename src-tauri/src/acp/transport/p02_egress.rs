use std::{
    io,
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Instant,
};

use tokio::{
    io::AsyncWrite,
    sync::Notify,
    time::{Instant as TokioInstant, sleep_until},
};

use super::{OutboundFrame, OutboundFrameClass};

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum P02EgressOutcome {
    Complete,
    ZeroByteFailure,
    EffectUnknown { accepted_bytes: NonZeroUsize },
}

#[derive(Clone, Default)]
pub(crate) struct P02Cancellation(Arc<P02CancellationInner>);

#[derive(Default)]
struct P02CancellationInner {
    cancelled: AtomicBool,
    notified: Notify,
}

#[allow(dead_code, reason = "P-02 sealed adapter cancellation")]
impl P02Cancellation {
    pub(crate) fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
        self.0.notified.notify_waiters();
    }

    fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if !self.is_cancelled() {
            self.0.notified.notified().await;
        }
    }
}

mod private {
    #[allow(dead_code, reason = "P-02 sealed adapter marker")]
    pub trait Sealed {}
}

/// The deliberately narrow P-02 physical egress seam.  It is sealed so a
/// closure or an unrelated adapter cannot manufacture delivery evidence.
#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
pub(crate) trait P02Egress: private::Sealed {
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>>;
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
pub(crate) struct DeadlineAsyncWriter<W> {
    output: W,
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
impl<W> DeadlineAsyncWriter<W> {
    pub(crate) fn new(output: W) -> Self {
        Self { output }
    }

    #[cfg(test)]
    pub(crate) fn into_inner(self) -> W {
        self.output
    }
}

impl<W: AsyncWrite + Unpin> private::Sealed for DeadlineAsyncWriter<W> {}
impl<W: AsyncWrite + Unpin> P02Egress for DeadlineAsyncWriter<W> {
    fn poll_write(&mut self, cx: &mut Context<'_>, bytes: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.output).poll_write(cx, bytes)
    }

    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.output).poll_flush(cx)
    }
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
pub(crate) async fn write_p02_response_until(
    egress: &mut impl P02Egress,
    frame: &OutboundFrame,
    deadline: Instant,
    cancellation: &P02Cancellation,
) -> P02EgressOutcome {
    if frame.class() != OutboundFrameClass::P02ReadTextFileResponse {
        return P02EgressOutcome::ZeroByteFailure;
    }
    let mut accepted = 0usize;
    for bytes in [frame.payload(), b"\n".as_slice()] {
        let mut offset = 0usize;
        while offset < bytes.len() {
            let offered = &bytes[offset..];
            let Some(result) = write_once_until(egress, offered, deadline, cancellation).await
            else {
                return outcome(accepted);
            };
            match result {
                Ok(0) if accepted == 0 => return P02EgressOutcome::ZeroByteFailure,
                Ok(0) | Err(_) => return outcome(accepted),
                Ok(count) if count > offered.len() => return outcome(accepted),
                Ok(count) => {
                    accepted = accepted
                        .checked_add(count)
                        .expect("bounded P-02 wire count");
                    offset += count;
                }
            }
        }
    }
    let flushed = flush_until(egress, deadline, cancellation).await;
    match flushed {
        Some(Ok(())) => P02EgressOutcome::Complete,
        Some(Err(_)) | None => outcome(accepted),
    }
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
async fn write_once_until(
    egress: &mut impl P02Egress,
    bytes: &[u8],
    deadline: Instant,
    cancellation: &P02Cancellation,
) -> Option<io::Result<usize>> {
    if Instant::now() >= deadline || cancellation.is_cancelled() {
        return None;
    }
    tokio::select! {
        biased;
        result = std::future::poll_fn(|cx| egress.poll_write(cx, bytes)) => Some(result),
        _ = cancellation.cancelled() => None,
        _ = sleep_until(TokioInstant::from_std(deadline)) => None,
    }
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
async fn flush_until(
    egress: &mut impl P02Egress,
    deadline: Instant,
    cancellation: &P02Cancellation,
) -> Option<io::Result<()>> {
    if Instant::now() >= deadline || cancellation.is_cancelled() {
        return None;
    }
    tokio::select! {
        biased;
        result = std::future::poll_fn(|cx| egress.poll_flush(cx)) => Some(result),
        _ = cancellation.cancelled() => None,
        _ = sleep_until(TokioInstant::from_std(deadline)) => None,
    }
}

#[allow(dead_code, reason = "P-02 sealed adapter egress entrypoint")]
fn outcome(accepted: usize) -> P02EgressOutcome {
    match NonZeroUsize::new(accepted) {
        Some(accepted_bytes) => P02EgressOutcome::EffectUnknown { accepted_bytes },
        None => P02EgressOutcome::ZeroByteFailure,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
        time::Duration,
    };

    struct Script {
        bytes: Vec<u8>,
        mode: Mode,
    }
    enum Mode {
        Complete,
        Zero,
        Partial,
        Delimiter,
        Flush,
        Pending,
    }
    impl AsyncWrite for Script {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            if matches!(self.mode, Mode::Pending) {
                return Poll::Pending;
            }
            if matches!(self.mode, Mode::Zero) {
                return Poll::Ready(Ok(0));
            }
            if matches!(self.mode, Mode::Delimiter) && bytes == b"\n" {
                return Poll::Ready(Err(io::Error::other("delimiter")));
            }
            let count = if matches!(self.mode, Mode::Partial) && self.bytes.is_empty() {
                1
            } else {
                bytes.len()
            };
            self.bytes.extend_from_slice(&bytes[..count]);
            Poll::Ready(Ok(count))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            if matches!(self.mode, Mode::Pending) {
                Poll::Pending
            } else if matches!(self.mode, Mode::Flush) {
                Poll::Ready(Err(io::Error::other("flush")))
            } else {
                Poll::Ready(Ok(()))
            }
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    fn frame() -> OutboundFrame {
        OutboundFrame::new_p02_read_text_file_response(br#"{"x":1}"#.to_vec()).unwrap()
    }
    #[tokio::test]
    async fn writes_payload_one_lf_and_flushes() {
        let mut writer = DeadlineAsyncWriter::new(Script {
            bytes: vec![],
            mode: Mode::Complete,
        });
        assert_eq!(
            write_p02_response_until(
                &mut writer,
                &frame(),
                Instant::now() + Duration::from_secs(1),
                &P02Cancellation::default()
            )
            .await,
            P02EgressOutcome::Complete
        );
        assert_eq!(writer.into_inner().bytes, b"{\"x\":1}\n");
    }
    #[tokio::test]
    async fn distinguishes_zero_and_positive_effects() {
        for (mode, expected) in [
            (Mode::Zero, true),
            (Mode::Partial, false),
            (Mode::Delimiter, false),
            (Mode::Flush, false),
        ] {
            let mut writer = DeadlineAsyncWriter::new(Script {
                bytes: vec![],
                mode,
            });
            let result = write_p02_response_until(
                &mut writer,
                &frame(),
                Instant::now() + Duration::from_secs(1),
                &P02Cancellation::default(),
            )
            .await;
            assert_eq!(
                matches!(result, P02EgressOutcome::ZeroByteFailure),
                expected
            );
        }
    }

    #[tokio::test]
    async fn cancellation_and_expiry_before_the_first_poll_prove_zero_bytes() {
        let cancellation = P02Cancellation::default();
        cancellation.cancel();
        let mut pending = DeadlineAsyncWriter::new(Script {
            bytes: vec![],
            mode: Mode::Pending,
        });
        assert_eq!(
            write_p02_response_until(
                &mut pending,
                &frame(),
                Instant::now() + Duration::from_secs(1),
                &cancellation
            )
            .await,
            P02EgressOutcome::ZeroByteFailure
        );
        let mut expired = DeadlineAsyncWriter::new(Script {
            bytes: vec![],
            mode: Mode::Pending,
        });
        assert_eq!(
            write_p02_response_until(
                &mut expired,
                &frame(),
                Instant::now() - Duration::from_millis(1),
                &P02Cancellation::default()
            )
            .await,
            P02EgressOutcome::ZeroByteFailure
        );
    }
}
