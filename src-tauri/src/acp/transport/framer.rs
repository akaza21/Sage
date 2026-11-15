use tokio::io::{AsyncRead, AsyncReadExt};

use super::oversize_id::{OversizeIdScanner, Scanned};
use sage_core::limits::LIMITS;

const READ_CHUNK_BYTES: usize = 8 * 1024;
const DISCARD_BYTES_MAX: usize = 8 * 1024 * 1024;
/// How far an id-less frame a harness may skip is read past. Reading past it
/// holds no more memory than the scanner's fixed state, so this only bounds a
/// harness that never ends a line; it sits well above the largest end-of-run
/// event a harness repeats its whole run in.
const SKIPPED_BYTES_MAX: usize = 256 * 1024 * 1024;

struct Discarding {
    scanner: OversizeIdScanner,
    bytes: usize,
}

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
    recoverable_oversize: bool,
    /// Oversized frames that carry no request id are skipped, not fatal.
    skip_oversize: bool,
    skipped_oversize: bool,
    discarding: Option<Box<Discarding>>,
    oversize_id: Option<u64>,
}

impl IncrementalFrameDecoder {
    pub(crate) fn new() -> Self {
        Self {
            assembly: Vec::new(),
            pending: Vec::new(),
            pending_cursor: 0,
            pending_cr: false,
            eof_seen: false,
            recoverable_oversize: false,
            skip_oversize: false,
            skipped_oversize: false,
            discarding: None,
            oversize_id: None,
        }
    }

    pub(crate) fn enable_recoverable_oversize(&mut self) {
        self.recoverable_oversize = true;
    }

    pub(crate) fn disable_recoverable_oversize(&mut self) {
        self.recoverable_oversize = false;
    }

    pub(crate) fn take_oversize_id(&mut self) -> Option<u64> {
        self.oversize_id.take()
    }

    pub(crate) fn set_oversize_skip(&mut self, skip: bool) {
        self.skip_oversize = skip;
    }

    pub(crate) fn take_skipped_oversize(&mut self) -> bool {
        std::mem::take(&mut self.skipped_oversize)
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
                Ok(None) if self.oversize_id.is_some() => return Ok(None),
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
        if self.assembly.is_empty() && !self.pending_cr && self.discarding.is_none() {
            Ok(None)
        } else {
            self.reset_frame();
            Err(FrameError::PartialEof)
        }
    }

    pub(crate) fn is_frame_aligned(&self) -> bool {
        !self.has_pending_input()
            && self.assembly.is_empty()
            && !self.pending_cr
            && self.discarding.is_none()
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        self.pending_cursor < self.pending.len()
    }

    #[allow(dead_code, reason = "adapter framing reset seam")]
    pub(crate) fn reset(&mut self) {
        self.assembly.clear();
        self.pending.clear();
        self.pending_cursor = 0;
        self.pending_cr = false;
        self.eof_seen = false;
        self.discarding = None;
        self.oversize_id = None;
        self.recoverable_oversize = false;
        self.skip_oversize = false;
        self.skipped_oversize = false;
    }

    fn accept_byte(&mut self, byte: u8) -> Result<Option<Frame>, FrameError> {
        if byte == b'\0' {
            self.reset_frame();
            return Err(FrameError::Nul);
        }
        if byte == b'\n' {
            self.pending_cr = false;
            if let Some(discarding) = self.discarding.take() {
                match discarding.scanner.finish() {
                    Scanned::Reply(id) => self.oversize_id = Some(id),
                    Scanned::Idless if self.skip_oversize => self.skipped_oversize = true,
                    Scanned::Idless | Scanned::Invalid => return Err(FrameError::Oversize),
                }
                return Ok(None);
            }
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
        if let Some(discarding) = &mut self.discarding {
            discarding.bytes += 1;
            let limit = if self.skip_oversize {
                SKIPPED_BYTES_MAX
            } else {
                DISCARD_BYTES_MAX
            };
            if discarding.bytes > limit || discarding.scanner.feed(byte).is_err() {
                self.reset_frame();
                return Err(FrameError::Oversize);
            }
            return Ok(None);
        }
        let maximum = frame_limit();
        if self.assembly.len() >= maximum {
            if !self.recoverable_oversize && !self.skip_oversize {
                self.reset_frame();
                return Err(FrameError::Oversize);
            }
            let mut scanner = OversizeIdScanner::new();
            for existing in &self.assembly {
                if scanner.feed(*existing).is_err() {
                    self.reset_frame();
                    return Err(FrameError::Oversize);
                }
            }
            if scanner.feed(byte).is_err() {
                self.reset_frame();
                return Err(FrameError::Oversize);
            }
            self.assembly.clear();
            self.discarding = Some(Box::new(Discarding { scanner, bytes: 1 }));
            return Ok(None);
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
        self.discarding = None;
    }

    #[cfg(test)]
    pub(super) fn capacities(&self) -> (usize, usize) {
        (self.assembly.capacity(), self.pending.capacity())
    }
}

#[allow(dead_code, reason = "adapter framing reader seam")]
pub(crate) struct FrameReader<R> {
    input: R,
    read_buffer: [u8; READ_CHUNK_BYTES],
    decoder: IncrementalFrameDecoder,
}

#[allow(dead_code, reason = "adapter framing reader seam")]
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

    #[test]
    fn recoverable_oversize_reports_exact_id_and_preserves_chunk_tail() {
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.enable_recoverable_oversize();
        let prefix = b"{\"id\":7,\"result\":{\"text\":\"";
        let suffix = b"\"}}\nnext\n";
        let frame = [
            prefix.as_slice(),
            &vec![b'x'; frame_limit()],
            suffix.as_slice(),
        ]
        .concat();
        let mut outputs = Vec::new();
        for chunk in frame.chunks(READ_CHUNK_BYTES) {
            if let Some(value) = decoder.push_chunk(chunk).unwrap() {
                outputs.push(value.as_bytes().to_vec());
            }
            if let Some(id) = decoder.take_oversize_id() {
                assert_eq!(id, 7);
                while decoder.has_pending_input() {
                    if let Some(value) = decoder.drain_pending().unwrap() {
                        outputs.push(value.as_bytes().to_vec());
                    }
                }
            }
        }
        assert_eq!(outputs, vec![b"next".to_vec()]);
    }

    #[test]
    fn skipped_oversize_drops_one_complete_idless_frame_and_keeps_reading() {
        let raw = format!(
            "{{\"type\":\"user\",\"content\":\"{}\"}}\nnext\n",
            "x".repeat(frame_limit())
        );
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.set_oversize_skip(true);
        let (mut frames, mut skipped) = (Vec::new(), 0);
        for chunk in raw.as_bytes().chunks(READ_CHUNK_BYTES) {
            let mut frame = decoder.push_chunk(chunk).unwrap();
            loop {
                skipped += usize::from(decoder.take_skipped_oversize());
                frames.extend(frame.map(|frame| frame.as_bytes().to_vec()));
                if !decoder.has_pending_input() {
                    break;
                }
                frame = decoder.drain_pending().unwrap();
            }
        }
        assert_eq!((frames, skipped), (vec![b"next".to_vec()], 1));
        let mut truncated = IncrementalFrameDecoder::new();
        truncated.set_oversize_skip(true);
        let broken = format!("{{\"type\":\"{}\n", "x".repeat(frame_limit()));
        let failed = broken
            .as_bytes()
            .chunks(READ_CHUNK_BYTES)
            .map(|chunk| truncated.push_chunk(chunk))
            .any(|result| matches!(result, Err(FrameError::Oversize)));
        assert!(failed);
        // A request is never skipped: its peer waits on an answer.
        let mut skipping = IncrementalFrameDecoder::new();
        skipping.set_oversize_skip(true);
        let request = format!(
            "{{\"id\":7,\"method\":\"x\",\"params\":\"{}\"}}\n",
            "x".repeat(frame_limit())
        );
        let refused = request
            .as_bytes()
            .chunks(READ_CHUNK_BYTES)
            .map(|chunk| skipping.push_chunk(chunk))
            .any(|result| matches!(result, Err(FrameError::Oversize)));
        assert!(refused);
    }

    #[test]
    fn skipped_oversize_reads_past_an_end_of_run_event_beyond_the_discard_bound() {
        let body = "x".repeat(DISCARD_BYTES_MAX + frame_limit());
        let raw = format!("{{\"type\":\"agent_end\",\"messages\":[\"{body}\"]}}\nnext\n");
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.set_oversize_skip(true);
        let (mut frames, mut skipped) = (Vec::new(), 0);
        for chunk in raw.as_bytes().chunks(READ_CHUNK_BYTES) {
            let mut frame = decoder.push_chunk(chunk).unwrap();
            loop {
                skipped += usize::from(decoder.take_skipped_oversize());
                frames.extend(frame.map(|frame| frame.as_bytes().to_vec()));
                assert!(decoder.assembly.capacity() <= frame_limit());
                if !decoder.has_pending_input() {
                    break;
                }
                frame = decoder.drain_pending().unwrap();
            }
        }
        assert_eq!((frames, skipped), (vec![b"next".to_vec()], 1));
    }

    fn decode_oversize(raw: &[u8]) -> Result<Option<u64>, FrameError> {
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.enable_recoverable_oversize();
        let mut observed = None;
        for chunk in raw.chunks(READ_CHUNK_BYTES) {
            let _ = decoder.push_chunk(chunk)?;
            if let Some(id) = decoder.take_oversize_id() {
                observed = Some(id);
            }
            while decoder.has_pending_input() {
                let _ = decoder.drain_pending()?;
                if let Some(id) = decoder.take_oversize_id() {
                    observed = Some(id);
                }
            }
        }
        decoder.finish()?;
        Ok(observed)
    }

    #[test]
    fn recoverable_oversize_rejects_missing_duplicate_and_non_numeric_ids() {
        for prefix in [
            r#"{"result":""#,
            r#"{"id":7,"id":8,"result":""#,
            r#"{"id":"7","result":""#,
            r#"{"id":null,"result":""#,
        ] {
            let raw = format!("{prefix}{}\"}}\n", "x".repeat(frame_limit()));
            assert_eq!(decode_oversize(raw.as_bytes()), Err(FrameError::Oversize));
        }
    }

    #[test]
    fn recoverable_oversize_proves_id_within_first_four_kib_only() {
        for (offset, expected) in [(4096, Ok(Some(7))), (4097, Err(FrameError::Oversize))] {
            let marker = r#"{"pad":"","id":7,"result":""#;
            let before_id = format!(
                "{{\"pad\":\"{}\",\"id\":7,\"result\":\"",
                "x".repeat(offset - marker.find('7').unwrap() - 1)
            );
            assert_eq!(before_id.find('7').unwrap() + 1, offset);
            let raw = format!("{before_id}{}\"}}\n", "x".repeat(frame_limit()));
            assert_eq!(decode_oversize(raw.as_bytes()), expected);
        }
    }

    #[test]
    fn recoverable_oversize_rejects_over_depth_and_partial_eof() {
        let nested_32 = format!(
            "{{\"id\":7,\"result\":{}\"{}\"{} }}\n",
            "[".repeat(31),
            "x".repeat(frame_limit()),
            "]".repeat(31)
        );
        assert_eq!(decode_oversize(nested_32.as_bytes()), Ok(Some(7)));
        let nested_33 = format!(
            "{{\"id\":7,\"result\":{}\"{}\"{} }}\n",
            "[".repeat(32),
            "x".repeat(frame_limit()),
            "]".repeat(32)
        );
        assert_eq!(
            decode_oversize(nested_33.as_bytes()),
            Err(FrameError::Oversize)
        );
        assert_eq!(
            decode_oversize(nested_32.trim_end().as_bytes()),
            Err(FrameError::PartialEof)
        );
    }

    #[test]
    fn recoverable_oversize_never_takes_a_request_for_a_reply() {
        // A server request carries an id from the peer's own counter, which
        // may equal one of Sage's pending ids; it stays a frame error.
        for (prefix, suffix) in [
            (
                r#"{"id":7,"method":"item/requestApproval","params":""#,
                r#""}"#,
            ),
            (r#"{"id":7,"params":""#, r#"","method":"x"}"#),
            (r#"{"id":7,"method":"x","params":""#, r#""}"#),
        ] {
            let raw = format!("{prefix}{}{suffix}\n", "x".repeat(frame_limit()));
            assert_eq!(decode_oversize(raw.as_bytes()), Err(FrameError::Oversize));
        }
        for prefix in [
            r#"{"id":7,"result":{"method":"x"},"text":""#,
            r#"{"id":7,"methods":"x","text":""#,
            r#"{"id":7,"metho":"x","text":""#,
            r#"{"id":7,"me\thod":"x","text":""#,
        ] {
            let raw = format!("{prefix}{}\"}}\n", "x".repeat(frame_limit()));
            assert_eq!(decode_oversize(raw.as_bytes()), Ok(Some(7)), "{prefix}");
        }
    }

    #[test]
    fn recoverable_oversize_accepts_escaped_id_key_and_crlf() {
        let raw = format!(
            "{{\"\\u0069d\":7,\"result\":\"{}\"}}\r\n",
            "x".repeat(frame_limit())
        );
        assert_eq!(decode_oversize(raw.as_bytes()), Ok(Some(7)));
    }

    #[test]
    fn discard_cap_is_exact_and_ordinary_notification_is_unchanged() {
        let prefix = b"{\"id\":7,\"result\":\"";
        let suffix = b"\"}\n";
        let allowed_text = frame_limit() + DISCARD_BYTES_MAX - prefix.len() - suffix.len() + 1;
        let allowed = [
            prefix.as_slice(),
            &vec![b'x'; allowed_text],
            suffix.as_slice(),
        ]
        .concat();
        assert_eq!(decode_oversize(&allowed), Ok(Some(7)));
        let excessive = [
            prefix.as_slice(),
            &vec![b'x'; allowed_text + 1],
            suffix.as_slice(),
        ]
        .concat();
        assert_eq!(decode_oversize(&excessive), Err(FrameError::Oversize));

        let ordinary = b"{\"method\":\"status\",\"params\":{}}\n";
        assert_eq!(decode_oversize(ordinary), Ok(None));
        let oversized_notification = format!(
            "{{\"method\":\"status\",\"value\":\"{}\"}}\n",
            "x".repeat(frame_limit())
        );
        assert_eq!(
            decode_oversize(oversized_notification.as_bytes()),
            Err(FrameError::Oversize)
        );
    }
}
