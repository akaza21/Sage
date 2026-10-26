use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

use super::{OUTPUT_BYTES, OUTPUT_CHUNK_BYTES, OUTPUT_LINES, OUTPUT_PAGE_BYTES, TerminalError};

#[derive(Clone)]
pub(super) struct TerminalOutputSink {
    ring: Arc<Mutex<OutputRing>>,
    changed: watch::Sender<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOutputSnapshot {
    pub bytes: Vec<u8>,
    pub truncations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOutputPage {
    pub bytes: Vec<u8>,
    pub next_cursor: u64,
    pub truncations: u64,
    pub truncated: bool,
}

#[derive(Default)]
struct OutputRing {
    bytes: VecDeque<u8>,
    lines: usize,
    truncations: u64,
}

impl TerminalOutputSink {
    pub(super) fn new() -> Self {
        let (changed, _) = watch::channel(0);
        Self {
            ring: Arc::new(Mutex::new(OutputRing::default())),
            changed,
        }
    }

    #[cfg_attr(
        not(any(windows, target_os = "linux", target_os = "macos")),
        allow(dead_code, reason = "only the PTY backends produce terminal output")
    )]
    pub(super) fn append(&self, chunk: &[u8]) -> Result<(), TerminalError> {
        if chunk.len() > OUTPUT_CHUNK_BYTES {
            return Err(TerminalError::Backend);
        }
        let mut ring = self.ring.lock().map_err(|_| TerminalError::Backend)?;
        for &byte in chunk {
            if byte == b'\n' {
                ring.lines += 1;
            }
            ring.bytes.push_back(byte);
            while ring.bytes.len() > OUTPUT_BYTES || ring.lines >= OUTPUT_LINES {
                if let Some(removed) = ring.bytes.pop_front() {
                    if removed == b'\n' {
                        ring.lines -= 1;
                    }
                    ring.truncations = ring.truncations.saturating_add(1);
                }
            }
        }
        let end = ring.truncations.saturating_add(ring.bytes.len() as u64);
        self.changed.send_replace(end);
        Ok(())
    }

    pub(super) fn subscribe(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub(super) fn wake(&self) {
        self.changed
            .send_modify(|value| *value = value.saturating_add(1));
    }

    pub(super) fn snapshot(&self) -> Result<TerminalOutputSnapshot, TerminalError> {
        let ring = self.ring.lock().map_err(|_| TerminalError::Backend)?;
        Ok(TerminalOutputSnapshot {
            bytes: ring.bytes.iter().copied().collect(),
            truncations: ring.truncations,
        })
    }

    pub(super) fn page(&self, cursor: u64) -> Result<TerminalOutputPage, TerminalError> {
        let ring = self.ring.lock().map_err(|_| TerminalError::Backend)?;
        let end = ring
            .truncations
            .checked_add(ring.bytes.len() as u64)
            .ok_or(TerminalError::Backend)?;
        if cursor > end {
            return Err(TerminalError::InvalidInput);
        }
        let truncated = cursor < ring.truncations;
        let start = cursor.max(ring.truncations);
        let offset = (start - ring.truncations) as usize;
        let bytes = ring
            .bytes
            .iter()
            .skip(offset)
            .take(OUTPUT_PAGE_BYTES)
            .copied()
            .collect::<Vec<_>>();
        Ok(TerminalOutputPage {
            next_cursor: start + bytes.len() as u64,
            bytes,
            truncations: ring.truncations,
            truncated,
        })
    }
}
