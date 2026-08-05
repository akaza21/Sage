// P1 freezes redaction types before P2 connects diagnostics and persistence.
#![allow(dead_code)]

use std::collections::VecDeque;

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum DiagnosticCode {
    ContainmentFailure,
    IdentityMismatch,
    InvalidLaunchSpec,
    PlatformUnavailable,
    ProtocolFailure,
    QueueCapacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SafeDiagnostic {
    pub(crate) code: DiagnosticCode,
    pub(crate) observed_bytes: u64,
    pub(crate) truncated: bool,
}

/// Sensitive bytes never implement `Debug`, `Display`, `Clone`, or `Serialize`.
/// Their allocation is cleared before it is released.
pub(crate) struct SensitiveBytes(Vec<u8>);

impl SensitiveBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub(crate) fn expose_to_normalizer(&self) -> &[u8] {
        &self.0
    }

    /// Discards the sensitive allocation and exposes only bounded diagnostic metadata.
    pub(crate) fn redact(self, code: DiagnosticCode) -> SafeDiagnostic {
        SafeDiagnostic {
            code,
            observed_bytes: u64::try_from(self.0.len()).unwrap_or(u64::MAX),
            truncated: false,
        }
    }
}

impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct ExternalIdFingerprint([u8; 16]);

impl ExternalIdFingerprint {
    pub(crate) fn from_external_id(id: &[u8]) -> Self {
        let digest = Sha256::digest(id);
        let mut prefix = [0; 16];
        prefix.copy_from_slice(&digest[..16]);
        Self(prefix)
    }
}

/// Retains bounded child stderr for native diagnostics only. No API returns its
/// content; sinks receive only a length/truncation summary.
pub(crate) struct StderrRing {
    bytes: VecDeque<u8>,
    byte_limit: usize,
    line_limit: usize,
    current_line_bytes: usize,
    observed_bytes: u64,
    truncated: bool,
}

impl StderrRing {
    pub(crate) fn new(byte_limit: usize, line_limit: usize) -> Self {
        Self {
            bytes: VecDeque::new(),
            byte_limit,
            line_limit,
            current_line_bytes: 0,
            observed_bytes: 0,
            truncated: false,
        }
    }

    pub(crate) fn ingest(&mut self, input: &[u8]) {
        self.observed_bytes = self
            .observed_bytes
            .saturating_add(u64::try_from(input.len()).unwrap_or(u64::MAX));
        for &byte in input {
            if byte == b'\n' {
                self.current_line_bytes = 0;
            } else {
                self.current_line_bytes = self.current_line_bytes.saturating_add(1);
                if self.current_line_bytes > self.line_limit {
                    self.truncated = true;
                    continue;
                }
            }
            if self.byte_limit == 0 {
                self.truncated = true;
                continue;
            }
            if self.bytes.len() == self.byte_limit {
                self.bytes.pop_front();
                self.truncated = true;
            }
            if self.bytes.len() == self.bytes.capacity() {
                let remaining = self.byte_limit.saturating_sub(self.bytes.capacity());
                if self.bytes.try_reserve_exact(remaining.min(4096)).is_err()
                    || self.bytes.capacity() > self.byte_limit
                {
                    self.bytes = VecDeque::new();
                    self.truncated = true;
                    continue;
                }
            }
            self.bytes.push_back(byte);
        }
    }

    pub(crate) fn safe_summary(&self) -> SafeDiagnostic {
        SafeDiagnostic {
            code: DiagnosticCode::ProtocolFailure,
            observed_bytes: self.observed_bytes,
            truncated: self.truncated,
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes.len()
    }
}

#[cfg(test)]
mod tests;
