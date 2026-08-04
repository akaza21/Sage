use std::io::Write;

use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::acp::limits::LIMITS;

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
///
/// Bytes remain private so callers cannot bypass the one-delimiter writer or
/// accidentally account only for the payload while retaining the LF too.
pub(crate) struct OutboundFrame(Vec<u8>);

impl OutboundFrame {
    pub(crate) fn new(payload: Vec<u8>) -> Result<Self, FrameWriteError> {
        validate_payload(&payload)?;
        Ok(Self(payload))
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn wire_len(&self) -> usize {
        self.0
            .len()
            .checked_add(1)
            .expect("validated ACP frame length plus delimiter fits usize")
    }
}

/// Writes one delimiter-free JSON payload as exactly one LF-delimited frame.
///
/// The outcome distinguishes a proven zero-byte failure (the sole retryable
/// delivery case) from every partial write or flush failure.
pub(crate) struct FrameWriter<W> {
    output: W,
}

pub(crate) fn write_json_line_blocking<W: Write>(
    output: &mut W,
    frame: &OutboundFrame,
) -> FrameWriteOutcome {
    let payload = frame.payload();
    let mut written = 0_usize;
    while written < payload.len() {
        match output.write(&payload[written..]) {
            Ok(0) | Err(_) if written == 0 => return FrameWriteOutcome::ZeroByteFailure,
            Ok(0) | Err(_) => {
                return FrameWriteOutcome::DeliveryUnknown {
                    bytes_written: written,
                };
            }
            Ok(count) => written += count,
        }
    }
    match output.write(b"\n") {
        Ok(1) => written += 1,
        Ok(_) | Err(_) => {
            return FrameWriteOutcome::DeliveryUnknown {
                bytes_written: written,
            };
        }
    }
    if output.flush().is_err() {
        return FrameWriteOutcome::DeliveryUnknown {
            bytes_written: written,
        };
    }
    FrameWriteOutcome::Complete
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    pub(crate) fn new(output: W) -> Self {
        Self { output }
    }

    pub(crate) async fn write_json_line(
        &mut self,
        frame: &OutboundFrame,
    ) -> Result<FrameWriteOutcome, FrameWriteError> {
        let payload = frame.payload();
        let mut written = 0_usize;
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

fn validate_payload(payload: &[u8]) -> Result<(), FrameWriteError> {
    if payload.is_empty() {
        return Err(FrameWriteError::Empty);
    }
    if payload.len() > LIMITS.wire.frame_bytes as usize {
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
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };

    use tokio::io::AsyncWrite;

    use super::*;

    struct FailingWriter {
        bytes: Vec<u8>,
        fail_after: usize,
    }

    impl AsyncWrite for FailingWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _context: &mut Context<'_>,
            buffer: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.bytes.len() >= self.fail_after {
                return Poll::Ready(Err(io::Error::other("injected")));
            }
            let count = buffer.len().min(self.fail_after - self.bytes.len());
            self.bytes.extend_from_slice(&buffer[..count]);
            Poll::Ready(Ok(count))
        }

        fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl io::Write for FailingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self.bytes.len() >= self.fail_after {
                return Err(io::Error::other("injected"));
            }
            let count = buffer.len().min(self.fail_after - self.bytes.len());
            self.bytes.extend_from_slice(&buffer[..count]);
            Ok(count)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn complete_write_adds_one_lf() {
        let output = FailingWriter {
            bytes: Vec::new(),
            fail_after: usize::MAX,
        };
        let mut writer = FrameWriter::new(output);
        let frame = OutboundFrame::new(br#"{"id":1}"#.to_vec()).unwrap();
        let outcome = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(writer.write_json_line(&frame))
            .unwrap();
        assert_eq!(outcome, FrameWriteOutcome::Complete);
        assert_eq!(writer.output.bytes, b"{\"id\":1}\n");
        assert_eq!(frame.wire_len(), frame.payload().len() + 1);
    }

    #[test]
    fn zero_and_partial_failures_have_distinct_delivery_evidence() {
        let mut zero = FrameWriter::new(FailingWriter {
            bytes: Vec::new(),
            fail_after: 0,
        });
        let frame = OutboundFrame::new(br#"{"id":1}"#.to_vec()).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(
            runtime.block_on(zero.write_json_line(&frame)).unwrap(),
            FrameWriteOutcome::ZeroByteFailure
        );

        let mut partial = FrameWriter::new(FailingWriter {
            bytes: Vec::new(),
            fail_after: 4,
        });
        assert_eq!(
            runtime.block_on(partial.write_json_line(&frame)).unwrap(),
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 4 }
        );
    }

    #[test]
    fn rejects_embedded_delimiters_before_writing() {
        let output = FailingWriter {
            bytes: Vec::new(),
            fail_after: usize::MAX,
        };
        assert_eq!(
            OutboundFrame::new(b"{}\n{}".to_vec()).err(),
            Some(FrameWriteError::InvalidDelimiter)
        );
        assert!(output.bytes.is_empty());
    }

    #[test]
    fn payload_limit_excludes_the_single_wire_delimiter() {
        let limit = usize::try_from(LIMITS.wire.frame_bytes).unwrap();
        let exact = OutboundFrame::new(vec![b' '; limit]).unwrap();
        assert_eq!(exact.wire_len(), limit + 1);
        assert_eq!(
            OutboundFrame::new(vec![b' '; limit + 1]).err(),
            Some(FrameWriteError::Oversize)
        );
    }

    #[test]
    fn blocking_writer_preserves_partial_delivery_evidence() {
        let frame = OutboundFrame::new(br#"{"id":1}"#.to_vec()).unwrap();
        let mut complete = FailingWriter {
            bytes: Vec::new(),
            fail_after: usize::MAX,
        };
        assert_eq!(
            write_json_line_blocking(&mut complete, &frame),
            FrameWriteOutcome::Complete
        );
        assert_eq!(complete.bytes, b"{\"id\":1}\n");

        let mut partial = FailingWriter {
            bytes: Vec::new(),
            fail_after: 3,
        };
        assert_eq!(
            write_json_line_blocking(&mut partial, &frame),
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 3 }
        );
    }
}
