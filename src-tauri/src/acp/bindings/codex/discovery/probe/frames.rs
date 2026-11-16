//! Decodes the probe's stdout into frames on the reader thread.

use std::{io::Read, sync::mpsc};

use crate::acp::transport::{Frame, FrameError, IncrementalFrameDecoder};

/// Sends each frame, then the end of stream or the first error, and stops.
pub(super) fn read_frames(
    mut stdout: impl Read,
    sender: &mpsc::SyncSender<Result<Option<Frame>, FrameError>>,
) {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let next = match decoder.drain_pending() {
            Ok(Some(frame)) => Ok(Some(frame)),
            Ok(None) => match stdout.read(&mut chunk) {
                Ok(0) => decoder.finish(),
                Ok(read) => match decoder.push_chunk(&chunk[..read]) {
                    Ok(None) => continue,
                    other => other,
                },
                Err(_) => Err(FrameError::Io),
            },
            Err(error) => Err(error),
        };
        let last = !matches!(next, Ok(Some(_)));
        if sender.send(next).is_err() || last {
            return;
        }
    }
}
