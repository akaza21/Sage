use tokio::io::{AsyncRead, AsyncReadExt};

use crate::acp::limits::LIMITS;

const READ_CHUNK_BYTES: usize = 8 * 1024;

/// A validated UTF-8 JSON frame. Raw bytes are deliberately private and this
/// type intentionally has no `Debug`, `Clone`, or serialization implementation.
pub(crate) struct Frame(Vec<u8>);

impl Frame {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameError {
    Io,
    InvalidUtf8,
    Nul,
    Oversize,
    PartialEof,
    UnexpectedDelimiter,
    ChunkCapacity,
    InputPending,
    AfterEof,
}

/// Incremental LF/CRLF decoder shared by async and blocking stdout readers.
///
/// `push_chunk`, `drain_pending`, and `finish` each yield at most one frame.
/// When `has_pending_input` is true, callers must drain until it becomes false
/// before pushing another chunk. Chunks are capped to the same 8 KiB size used
/// by `FrameReader`, so retaining the tail after a multi-frame read is bounded.
pub(crate) struct IncrementalFrameDecoder {
    assembly: Vec<u8>,
    pending: Vec<u8>,
    pending_cursor: usize,
    pending_cr: bool,
    eof_seen: bool,
}

impl IncrementalFrameDecoder {
    pub(crate) fn new() -> Self {
        Self {
            assembly: Vec::new(),
            pending: Vec::new(),
            pending_cursor: 0,
            pending_cr: false,
            eof_seen: false,
        }
    }

    pub(crate) fn push_chunk(&mut self, chunk: &[u8]) -> Result<Option<Frame>, FrameError> {
        if self.eof_seen {
            return Err(FrameError::AfterEof);
        }
        if self.has_pending_input() {
            return Err(FrameError::InputPending);
        }
        if chunk.len() > READ_CHUNK_BYTES {
            return Err(FrameError::ChunkCapacity);
        }
        self.pending.clear();
        self.pending_cursor = 0;
        if self.pending.capacity() < chunk.len() {
            let additional = chunk.len() - self.pending.capacity();
            self.pending
                .try_reserve_exact(additional)
                .map_err(|_| FrameError::Io)?;
            if self.pending.capacity() > READ_CHUNK_BYTES {
                self.pending = Vec::new();
                return Err(FrameError::Io);
            }
        }
        self.pending.extend_from_slice(chunk);
        self.drain_pending()
    }

    pub(crate) fn drain_pending(&mut self) -> Result<Option<Frame>, FrameError> {
        while self.pending_cursor < self.pending.len() {
            let byte = self.pending[self.pending_cursor];
            self.pending_cursor += 1;
            match self.accept_byte(byte) {
                Ok(Some(frame)) => return Ok(Some(frame)),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
        }
        self.pending.clear();
        self.pending_cursor = 0;
        Ok(None)
    }

    pub(crate) fn finish(&mut self) -> Result<Option<Frame>, FrameError> {
        self.eof_seen = true;
        if let Some(frame) = self.drain_pending()? {
            return Ok(Some(frame));
        }
        if self.assembly.is_empty() && !self.pending_cr {
            Ok(None)
        } else {
            self.reset_frame();
            Err(FrameError::PartialEof)
        }
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        self.pending_cursor < self.pending.len()
    }

    /// Discards all buffered state so a caller can explicitly reuse the
    /// decoder after terminating a malformed stream.
    pub(crate) fn reset(&mut self) {
        self.assembly.clear();
        self.pending.clear();
        self.pending_cursor = 0;
        self.pending_cr = false;
        self.eof_seen = false;
    }

    fn accept_byte(&mut self, byte: u8) -> Result<Option<Frame>, FrameError> {
        if byte == b'\0' {
            self.reset_frame();
            return Err(FrameError::Nul);
        }
        if byte == b'\n' {
            self.pending_cr = false;
            if std::str::from_utf8(&self.assembly).is_err() {
                self.reset_frame();
                return Err(FrameError::InvalidUtf8);
            }
            return Ok(Some(Frame(std::mem::take(&mut self.assembly))));
        }
        if self.pending_cr {
            self.reset_frame();
            return Err(FrameError::UnexpectedDelimiter);
        }
        if byte == b'\r' {
            self.pending_cr = true;
            return Ok(None);
        }
        let maximum = frame_limit();
        if self.assembly.len() >= maximum {
            self.reset_frame();
            return Err(FrameError::Oversize);
        }
        self.reserve_for_next(maximum)?;
        self.assembly.push(byte);
        Ok(None)
    }

    fn reserve_for_next(&mut self, maximum: usize) -> Result<(), FrameError> {
        if self.assembly.len() < self.assembly.capacity() {
            return Ok(());
        }
        let remaining = maximum.saturating_sub(self.assembly.capacity());
        if remaining == 0 {
            return Err(FrameError::Oversize);
        }
        self.assembly
            .try_reserve_exact(remaining.min(READ_CHUNK_BYTES))
            .map_err(|_| FrameError::Io)?;
        if self.assembly.capacity() > maximum {
            self.assembly = Vec::new();
            return Err(FrameError::Io);
        }
        Ok(())
    }

    fn reset_frame(&mut self) {
        self.assembly.clear();
        self.pending_cr = false;
    }

    #[cfg(test)]
    pub(super) fn capacities(&self) -> (usize, usize) {
        (self.assembly.capacity(), self.pending.capacity())
    }
}

pub(crate) struct FrameReader<R> {
    input: R,
    read_buffer: [u8; READ_CHUNK_BYTES],
    decoder: IncrementalFrameDecoder,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(crate) fn new(input: R) -> Self {
        Self {
            input,
            read_buffer: [0; READ_CHUNK_BYTES],
            decoder: IncrementalFrameDecoder::new(),
        }
    }

    pub(crate) async fn read_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        loop {
            if let Some(frame) = self.decoder.drain_pending()? {
                return Ok(Some(frame));
            }
            let read = self
                .input
                .read(&mut self.read_buffer)
                .await
                .map_err(|_| FrameError::Io)?;
            if read == 0 {
                return self.decoder.finish();
            }
            if let Some(frame) = self.decoder.push_chunk(&self.read_buffer[..read])? {
                return Ok(Some(frame));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn assembly_capacity(&self) -> usize {
        self.decoder.capacities().0
    }
}

fn frame_limit() -> usize {
    usize::try_from(LIMITS.wire.frame_bytes).expect("wire frame limit fits usize")
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[test]
    fn async_reader_delegates_fragmented_multi_frame_state_to_decoder() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut writer, input) = tokio::io::duplex(128);
                writer.write_all(b"fir").await.unwrap();
                writer.write_all(b"st\r\nsecond\n").await.unwrap();
                drop(writer);
                let mut reader = FrameReader::new(input);
                assert_eq!(
                    reader.read_frame().await.unwrap().unwrap().as_bytes(),
                    b"first"
                );
                assert_eq!(
                    reader.read_frame().await.unwrap().unwrap().as_bytes(),
                    b"second"
                );
                assert!(reader.read_frame().await.unwrap().is_none());
                assert!(reader.assembly_capacity() <= frame_limit());
            });
    }

    #[test]
    fn split_crlf_and_multiple_frames_use_bounded_drain_without_loss() {
        let mut decoder = IncrementalFrameDecoder::new();
        assert!(decoder.push_chunk(b"one\r").unwrap().is_none());
        let first = decoder.push_chunk(b"\ntwo\nthree\n").unwrap().unwrap();
        assert_eq!(first.as_bytes(), b"one");
        assert!(decoder.has_pending_input());
        assert_eq!(
            decoder.push_chunk(b"not-consumed").err(),
            Some(FrameError::InputPending)
        );
        let second = decoder.drain_pending().unwrap().unwrap();
        let third = decoder.drain_pending().unwrap().unwrap();
        assert_eq!(second.as_bytes(), b"two");
        assert_eq!(third.as_bytes(), b"three");
        assert!(decoder.drain_pending().unwrap().is_none());
        assert!(!decoder.has_pending_input());
    }

    #[test]
    fn exact_frame_boundary_succeeds_and_plus_one_resets() {
        let limit = frame_limit();
        let mut decoder = IncrementalFrameDecoder::new();
        for chunk in vec![b'x'; limit].chunks(READ_CHUNK_BYTES) {
            assert!(decoder.push_chunk(chunk).unwrap().is_none());
        }
        let frame = decoder.push_chunk(b"\n").unwrap().unwrap();
        assert_eq!(frame.len(), limit);

        for chunk in vec![b'x'; limit].chunks(READ_CHUNK_BYTES) {
            assert!(decoder.push_chunk(chunk).unwrap().is_none());
        }
        assert_eq!(decoder.push_chunk(b"y").err(), Some(FrameError::Oversize));
        assert!(decoder.push_chunk(b"ok\n").unwrap().is_some());
    }

    #[test]
    fn nul_utf8_and_unexpected_cr_reset_only_the_current_frame() {
        for (malformed, error) in [
            (&b"bad\0ok\n"[..], FrameError::Nul),
            (
                &[0xff, b'\n', b'o', b'k', b'\n'][..],
                FrameError::InvalidUtf8,
            ),
            (&b"bad\rXok\n"[..], FrameError::UnexpectedDelimiter),
        ] {
            let mut decoder = IncrementalFrameDecoder::new();
            assert_eq!(decoder.push_chunk(malformed).err(), Some(error));
            assert_eq!(decoder.drain_pending().unwrap().unwrap().as_bytes(), b"ok");
            decoder.reset();
            assert_eq!(
                decoder.push_chunk(b"fresh\n").unwrap().unwrap().as_bytes(),
                b"fresh"
            );
        }
    }

    #[test]
    fn explicit_eof_rejects_partial_frames_and_then_is_terminal() {
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.push_chunk(b"partial").unwrap();
        assert_eq!(decoder.finish().err(), Some(FrameError::PartialEof));
        assert!(decoder.finish().unwrap().is_none());
        assert_eq!(
            decoder.push_chunk(b"late\n").err(),
            Some(FrameError::AfterEof)
        );
        decoder.reset();
        assert_eq!(
            decoder.push_chunk(b"fresh\n").unwrap().unwrap().as_bytes(),
            b"fresh"
        );
    }

    #[test]
    fn chunk_and_internal_capacities_are_mechanically_bounded() {
        let mut decoder = IncrementalFrameDecoder::new();
        assert_eq!(
            decoder.push_chunk(&vec![b'x'; READ_CHUNK_BYTES + 1]).err(),
            Some(FrameError::ChunkCapacity)
        );
        decoder.push_chunk(&vec![b'x'; READ_CHUNK_BYTES]).unwrap();
        let (assembly, pending) = decoder.capacities();
        assert!(assembly <= frame_limit());
        assert!(pending <= READ_CHUNK_BYTES);
    }
}
