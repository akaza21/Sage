use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::acp::limits::{LIMITS, preview::P02_READ_TEXT_RESPONSE_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameWriteError {
    Empty,
    InvalidDelimiter,
    InvalidUtf8,
    Nul,
    Oversize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameWriteOutcome {
    Complete,
    ZeroByteFailure,
    DeliveryUnknown { bytes_written: usize },
}

/// A validated, delimiter-free outbound JSON frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code, reason = "P-02 sealed frame class")]
pub(crate) enum OutboundFrameClass {
    Ordinary,
    P02ReadTextFileResponse,
}

#[allow(dead_code, reason = "ACP transport frame")]
pub(crate) struct OutboundFrame {
    payload: Vec<u8>,
    class: OutboundFrameClass,
}

#[allow(dead_code, reason = "ACP transport frame")]
impl OutboundFrame {
    pub(crate) fn new(payload: Vec<u8>) -> Result<Self, FrameWriteError> {
        Self::new_with_class(payload, OutboundFrameClass::Ordinary)
    }

    pub(crate) fn new_p02_read_text_file_response(
        payload: Vec<u8>,
    ) -> Result<Self, FrameWriteError> {
        Self::new_with_class(payload, OutboundFrameClass::P02ReadTextFileResponse)
    }

    fn new_with_class(
        payload: Vec<u8>,
        class: OutboundFrameClass,
    ) -> Result<Self, FrameWriteError> {
        validate_payload(&payload, class)?;
        Ok(Self { payload, class })
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub(crate) fn wire_len(&self) -> usize {
        self.payload
            .len()
            .checked_add(1)
            .expect("validated frame length")
    }
    pub(crate) fn class(&self) -> OutboundFrameClass {
        self.class
    }
}

/// Ordinary ACP transport writer. P-02 uses `p02_egress` instead so it can
/// apply one absolute deadline to every physical poll without changing this
/// established 1 MiB path.
#[allow(dead_code, reason = "ordinary adapter writer seam")]
pub(crate) struct FrameWriter<W> {
    output: W,
}

#[allow(dead_code, reason = "ordinary adapter writer seam")]
impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub(crate) fn new(output: W) -> Self {
        Self { output }
    }

    pub(crate) async fn write_json_line(
        &mut self,
        frame: &OutboundFrame,
    ) -> Result<FrameWriteOutcome, FrameWriteError> {
        if frame.class() == OutboundFrameClass::P02ReadTextFileResponse {
            return Ok(FrameWriteOutcome::ZeroByteFailure);
        }
        let payload = frame.payload();
        let mut written = 0usize;
        while written < payload.len() {
            match self.output.write(&payload[written..]).await {
                Ok(0) | Err(_) if written == 0 => return Ok(FrameWriteOutcome::ZeroByteFailure),
                Ok(0) | Err(_) => {
                    return Ok(FrameWriteOutcome::DeliveryUnknown {
                        bytes_written: written,
                    });
                }
                Ok(count) => written += count,
            }
        }
        match self.output.write(b"\n").await {
            Ok(1) => written += 1,
            Ok(_) | Err(_) => {
                return Ok(FrameWriteOutcome::DeliveryUnknown {
                    bytes_written: written,
                });
            }
        }
        if self.output.flush().await.is_err() {
            return Ok(FrameWriteOutcome::DeliveryUnknown {
                bytes_written: written,
            });
        }
        Ok(FrameWriteOutcome::Complete)
    }
}

fn validate_payload(payload: &[u8], class: OutboundFrameClass) -> Result<(), FrameWriteError> {
    if payload.is_empty() {
        return Err(FrameWriteError::Empty);
    }
    let limit = match class {
        OutboundFrameClass::Ordinary => LIMITS.wire.frame_bytes as usize,
        OutboundFrameClass::P02ReadTextFileResponse => P02_READ_TEXT_RESPONSE_BYTES,
    };
    if payload.len() > limit {
        return Err(FrameWriteError::Oversize);
    }
    if payload.contains(&b'\0') {
        return Err(FrameWriteError::Nul);
    }
    if payload.contains(&b'\r') || payload.contains(&b'\n') {
        return Err(FrameWriteError::InvalidDelimiter);
    }
    std::str::from_utf8(payload).map_err(|_| FrameWriteError::InvalidUtf8)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::AsyncWrite;
    struct Sink;
    impl AsyncWrite for Sink {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    #[test]
    fn only_p02_can_exceed_ordinary_limit() {
        let payload = vec![b' '; LIMITS.wire.frame_bytes as usize + 1];
        assert_eq!(
            OutboundFrame::new(payload.clone()).err(),
            Some(FrameWriteError::Oversize)
        );
        assert_eq!(
            OutboundFrame::new_p02_read_text_file_response(payload)
                .unwrap()
                .class(),
            OutboundFrameClass::P02ReadTextFileResponse
        );
    }
    #[tokio::test]
    async fn ordinary_writer_keeps_the_ordinary_frame_path() {
        let mut writer = FrameWriter::new(Sink);
        assert_eq!(
            writer
                .write_json_line(&OutboundFrame::new(b"{}".to_vec()).unwrap())
                .await
                .unwrap(),
            FrameWriteOutcome::Complete
        );
    }
    #[test]
    fn delimiter_is_rejected() {
        assert_eq!(
            OutboundFrame::new(b"{}\n".to_vec()).err(),
            Some(FrameWriteError::InvalidDelimiter)
        );
    }
}

#[cfg(test)]
#[tokio::test]
async fn ordinary_writer_rejects_p02_without_writing() {
    let mut writer = FrameWriter::new(Vec::<u8>::new());
    let frame = OutboundFrame::new_p02_read_text_file_response(b"{}".to_vec()).unwrap();
    assert_eq!(
        writer.write_json_line(&frame).await.unwrap(),
        FrameWriteOutcome::ZeroByteFailure
    );
    assert!(writer.output.is_empty());
}
