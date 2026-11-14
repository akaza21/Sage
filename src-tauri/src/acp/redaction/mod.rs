use std::collections::VecDeque;

use serde::Serialize;
use serde_json::Value;

#[cfg(test)]
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    dead_code,
    reason = "P2: codes are frozen before diagnostics are connected; only ProtocolFailure is produced yet"
)]
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

/// A bounded, structured child-stderr observation. It intentionally exposes
/// nothing for unstructured stderr, which can contain arbitrary secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SafeStderrDiagnostic {
    pub(crate) method: Option<String>,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
}

/// Sensitive bytes never implement `Debug`, `Display`, `Clone`, or `Serialize`.
/// Their allocation is cleared before it is released.
#[cfg(test)]
pub(crate) struct SensitiveBytes(Vec<u8>);

#[cfg(test)]
impl SensitiveBytes {
    pub(crate) fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub(crate) fn expose_to_normalizer(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn redact(self, code: DiagnosticCode) -> SafeDiagnostic {
        SafeDiagnostic {
            code,
            observed_bytes: u64::try_from(self.0.len()).unwrap_or(u64::MAX),
            truncated: false,
        }
    }
}

#[cfg(test)]
impl Drop for SensitiveBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct ExternalIdFingerprint([u8; 16]);

#[cfg(test)]
impl ExternalIdFingerprint {
    pub(crate) fn from_external_id(id: &[u8]) -> Self {
        let digest = Sha256::digest(id);
        let mut prefix = [0; 16];
        prefix.copy_from_slice(&digest[..16]);
        Self(prefix)
    }
}

/// Retains bounded child stderr for native diagnostics only. Sinks receive
/// either its length/truncation summary or a strictly structured, sanitized
/// JSON-RPC error diagnostic.
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

    pub(crate) fn safe_diagnostic(&self) -> SafeStderrDiagnostic {
        let bytes = self.bytes.iter().copied().collect::<Vec<_>>();
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes)
            && let Some(object) = value.as_object()
            && let Some(error) = object.get("error").and_then(Value::as_object)
        {
            let method = object
                .get("method")
                .and_then(Value::as_str)
                .filter(|method| method.len() <= 128)
                .map(str::to_owned);
            let error_code = error.get("code").and_then(|code| match code {
                Value::Number(code) => Some(code.to_string()),
                Value::String(code) => Some(sanitize_diagnostic_text(code)),
                _ => None,
            });
            let error_message = error
                .get("message")
                .and_then(Value::as_str)
                .filter(|message| message.len() <= 1024)
                .map(sanitize_diagnostic_text);
            return SafeStderrDiagnostic {
                method,
                error_code,
                error_message,
            };
        }
        std::str::from_utf8(&bytes)
            .ok()
            .filter(|message| message.len() <= 1024)
            .map(|message| SafeStderrDiagnostic {
                method: None,
                error_code: Some("child-stderr".to_owned()),
                error_message: Some(sanitize_diagnostic_text(message)),
            })
            .unwrap_or_else(empty_stderr_diagnostic)
    }

    #[cfg(all(test, feature = "acp-fixture"))]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes.len()
    }
}

fn empty_stderr_diagnostic() -> SafeStderrDiagnostic {
    SafeStderrDiagnostic {
        method: None,
        error_code: None,
        error_message: None,
    }
}

mod diagnostic;
pub(crate) mod text;
pub(crate) use diagnostic::sanitize_diagnostic_text;

#[cfg(test)]
mod tests;
