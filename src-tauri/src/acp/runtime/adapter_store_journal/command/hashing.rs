use std::io::Write;

use sha2::{Digest, Sha256};

use crate::acp::limits::LIMITS;

#[derive(Default)]
pub(super) struct CountingDigest {
    bytes: usize,
    digest: Sha256,
}

impl CountingDigest {
    pub(super) fn matched(self, expected: &str) -> Option<usize> {
        (self.bytes <= LIMITS.wire.normalized_item_bytes as usize
            && hex(&self.digest.finalize()) == expected)
            .then_some(self.bytes)
    }
}

impl Write for CountingDigest {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= LIMITS.wire.normalized_item_bytes as usize)
            .ok_or_else(|| std::io::Error::other("normalized payload capacity"))?;
        self.digest.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(in crate::acp::runtime::adapter_store_journal) fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(in crate::acp::runtime::adapter_store_journal) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
