use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use super::{OUTPUT_BYTES, OUTPUT_CHUNK_BYTES, OUTPUT_LINES, TerminalError};

#[derive(Clone)]
pub(super) struct TerminalOutputSink(Arc<Mutex<OutputRing>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalOutputSnapshot {
    pub bytes: Vec<u8>,
    pub truncations: u64,
}

#[derive(Default)]
struct OutputRing {
    bytes: VecDeque<u8>,
    lines: usize,
    truncations: u64,
}

impl TerminalOutputSink {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(OutputRing::default())))
    }

    pub(super) fn append(&self, chunk: &[u8]) -> Result<(), TerminalError> {
        if chunk.len() > OUTPUT_CHUNK_BYTES {
            return Err(TerminalError::Backend);
        }
        let mut ring = self.0.lock().map_err(|_| TerminalError::Backend)?;
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
        Ok(())
    }

    pub(super) fn snapshot(&self) -> Result<TerminalOutputSnapshot, TerminalError> {
        let ring = self.0.lock().map_err(|_| TerminalError::Backend)?;
        Ok(TerminalOutputSnapshot {
            bytes: ring.bytes.iter().copied().collect(),
            truncations: ring.truncations,
        })
    }
}
