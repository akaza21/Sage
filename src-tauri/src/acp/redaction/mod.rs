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
    json_rpc: bool,
}

impl SafeStderrDiagnostic {
    /// The harness's JSON-RPC error message. A sanitized plain-text stderr
    /// line stays a native diagnostic and is never handed out for storage.
    pub(crate) fn json_rpc_error_message(&self) -> Option<&str> {
        self.json_rpc
            .then_some(self.error_message.as_deref())
            .flatten()
    }
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
    /// The oldest retained bytes continue a line whose start was evicted.
    partial_front: bool,
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
            partial_front: false,
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
                self.partial_front = self.bytes.pop_front() != Some(b'\n');
                self.truncated = true;
            }
            if self.bytes.len() == self.bytes.capacity() {
                let remaining = self.byte_limit.saturating_sub(self.bytes.capacity());
                if self.bytes.try_reserve_exact(remaining.min(4096)).is_err()
                    || self.bytes.capacity() > self.byte_limit
                {
                    self.bytes = VecDeque::new();
                    self.truncated = true;
                    self.partial_front = byte != b'\n';
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

    /// The latest complete JSON-RPC error record wins: a harness writes its
    /// error after any startup log and may report more than one. A line cut
    /// by ring eviction is never read as a record.
    pub(crate) fn safe_diagnostic(&self) -> SafeStderrDiagnostic {
        let bytes = self.bytes.iter().copied().collect::<Vec<_>>();
        let complete = if self.partial_front {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(&[][..], |end| &bytes[end + 1..])
        } else {
            &bytes[..]
        };
        if let Some(diagnostic) = complete
            .split(|byte| *byte == b'\n')
            .rev()
            .find_map(json_rpc_diagnostic)
            .or_else(|| json_rpc_diagnostic(complete))
        {
            return diagnostic;
        }
        std::str::from_utf8(&bytes)
            .ok()
            .filter(|message| message.len() <= 1024)
            .map(|message| SafeStderrDiagnostic {
                method: None,
                error_code: Some("child-stderr".to_owned()),
                error_message: Some(sanitize_diagnostic_text(message)),
                json_rpc: false,
            })
            .unwrap_or_else(empty_stderr_diagnostic)
    }

    #[cfg(all(test, feature = "acp-fixture"))]
    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes.len()
    }
}

/// Admits only a JSON-RPC 2.0 error envelope: `"jsonrpc":"2.0"`, a response
/// `id` (number, string or null) or a `method` name, and an error with an
/// integer code and a string message. Any other structured log, however it
/// names its fields, stays an unstructured native diagnostic.
fn json_rpc_diagnostic(record: &[u8]) -> Option<SafeStderrDiagnostic> {
    let value = serde_json::from_slice::<Value>(record).ok()?;
    let object = value.as_object()?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return None;
    }
    let method = match object.get("method") {
        None => None,
        Some(Value::String(method)) => Some(method),
        Some(_) => return None,
    };
    let id_shaped = match object.get("id") {
        None => false,
        Some(Value::Number(_) | Value::String(_) | Value::Null) => true,
        Some(_) => return None,
    };
    if !id_shaped && method.is_none() {
        return None;
    }
    let error = object.get("error").and_then(Value::as_object)?;
    let code = error.get("code").and_then(Value::as_i64)?;
    let message = error.get("message").and_then(Value::as_str)?;
    Some(SafeStderrDiagnostic {
        method: method
            .filter(|method| method.len() <= 128)
            .map(String::to_owned),
        error_code: Some(code.to_string()),
        error_message: Some(message)
            .filter(|message| message.len() <= 1024)
            .map(sanitize_diagnostic_text),
        json_rpc: true,
    })
}

fn empty_stderr_diagnostic() -> SafeStderrDiagnostic {
    SafeStderrDiagnostic {
        method: None,
        error_code: None,
        error_message: None,
        json_rpc: false,
    }
}

mod diagnostic;
pub(crate) mod text;
pub(crate) use diagnostic::sanitize_diagnostic_text;

#[cfg(test)]
mod tests;
