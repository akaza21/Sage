#![deny(dead_code)]
//! P-02's harness-neutral, read-only source primitive.
//!
//! Deferred bindings use the typed request seam below for conformance only.
//! It is deliberately not installed on a live lifecycle/pump (Decision B).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    acp::service::{ReadTextBegin, ReadTextFileService, ReadTextOwner},
    acp::{
        limits::preview::P02_READ_TEXT_SOURCE_BYTES,
        resource_mapper::NativeResourceAdmission,
        transport::{FrameWriteError, OutboundFrame},
    },
    domain::ResourceRef,
    error::AppResult,
    workspace::{Workspace, select_text_lines},
};

#[allow(
    dead_code,
    reason = "P-02 Decision B deferred binding ingress seam; no live adapter pump is admitted"
)]
pub(crate) const ACP_READ_TEXT_FILE_METHOD: &str = "fs/read_text_file";
const MAX_REQUEST_ID_BYTES: usize = 512;
const MAX_PATH_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadTextRange {
    pub(crate) line: Option<u64>,
    pub(crate) limit: Option<u64>,
}

/// ACP v1 pins JSON-RPC request IDs to a bounded string or signed i64.  Keep
/// the parsed type so a response never stringifies a numeric correlation ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadTextRequestId {
    String(String),
    Number(i64),
}
impl ReadTextRequestId {
    fn from_value(value: Value) -> Result<Self, &'static str> {
        match value {
            Value::String(value)
                if !value.is_empty()
                    && value.len() <= MAX_REQUEST_ID_BYTES
                    && !value.contains('\0') =>
            {
                Ok(Self::String(value))
            }
            Value::Number(value) => value
                .as_i64()
                .map(Self::Number)
                .ok_or("invalid JSON-RPC id"),
            _ => Err("invalid JSON-RPC id"),
        }
    }
    #[allow(dead_code, reason = "P-02 sealed egress encoder is adapter-deferred")]
    pub(crate) fn as_value(&self) -> Value {
        match self {
            Self::String(value) => Value::String(value.clone()),
            Self::Number(value) => Value::Number((*value).into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReadTextFileHandoff {
    pub(crate) request_id: ReadTextRequestId,
    /// ACP v1 requires the external session identity on every request.
    pub(crate) session_id: String,
    /// ACP v1 supplies an absolute native path. It is resolved immediately to
    /// an owner-bound canonical resource and is never put in a receipt.
    pub(crate) absolute_path: String,
    pub(crate) range: ReadTextRange,
    /// ACP extensibility is an opaque object, bounded by adapter preflight.
    pub(crate) meta: Option<Map<String, Value>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireReadParams {
    session_id: String,
    path: String,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default, rename = "_meta")]
    meta: Option<Map<String, Value>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireReadRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    params: WireReadParams,
}

/// Decode a request only after its adapter's bounded duplicate-key/depth/node
/// wire preflight has produced `value`. `_meta` remains opaque, but bounded by
/// that preflight and this 16 KiB serialization cap.
pub(crate) fn decode_read_text_file_value(
    value: Value,
) -> Result<ReadTextFileHandoff, &'static str> {
    let request: WireReadRequest =
        serde_json::from_value(value).map_err(|_| "malformed request")?;
    let meta_bytes = request
        .params
        .meta
        .as_ref()
        .map(serde_json::to_vec)
        .transpose()
        .map_err(|_| "invalid metadata")?
        .map_or(0, |bytes| bytes.len());
    if request.jsonrpc != "2.0"
        || request.method != ACP_READ_TEXT_FILE_METHOD
        || request.params.session_id.is_empty()
        || request.params.session_id.len() > MAX_REQUEST_ID_BYTES
        || request.params.session_id.contains('\0')
        || request.params.path.is_empty()
        || request.params.path.len() > MAX_PATH_BYTES
        || request.params.path.contains('\0')
        || !is_native_absolute(&request.params.path)
        || request.params.line == Some(0)
        || request.params.limit == Some(0)
        || meta_bytes > 16 * 1024
    {
        return Err("invalid read-text request");
    }
    Ok(ReadTextFileHandoff {
        request_id: ReadTextRequestId::from_value(request.id)?,
        session_id: request.params.session_id,
        absolute_path: request.params.path,
        range: ReadTextRange {
            line: request.params.line.map(u64::from),
            limit: request.params.limit.map(u64::from),
        },
        meta: request.params.meta,
    })
}

/// Immutable authority expected at a concrete deferred-runtime boundary.
/// The runtime supplies its binding/session/epoch; its owner scope is provided
/// by the already-established Sage session record, never inferred from wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedOwnerScope {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) owner: ReadTextOwner,
}

impl ExpectedOwnerScope {
    pub(crate) fn matches_runtime(
        &self,
        binding_digest: &[u8; 32],
        session_id: &crate::acp::contract::SageAgentSessionId,
        runtime_epoch: crate::acp::contract::RuntimeEpoch,
    ) -> bool {
        self.binding_digest == *binding_digest
            && self.owner.session_id == *session_id
            && self.owner.runtime_epoch == runtime_epoch
    }
}

/// Adapter ingress resolves only an already-bound external session against an
/// exact immutable scope. Returning a different owner is rejected by the
/// shared route as a defense in depth check.
pub(crate) trait DeferredReadTextOwnerResolver {
    fn owner_for_external_session(
        &self,
        external_session: &str,
        expected: &ExpectedOwnerScope,
    ) -> Option<ReadTextOwner>;
}

/// A typed handoff proves one adapter request reached the shared owner-bound
/// service. The receiver remains deferred until an admitted transport owns its
/// bounded deadline and response write.
#[allow(
    dead_code,
    reason = "P-02 Decision B deferred typed handoff is not production-admitted"
)]
pub(crate) struct DeferredReadTextFile {
    pub(crate) request_id: ReadTextRequestId,
    pub(crate) begin: ReadTextBegin,
}

pub(crate) fn route_deferred_read_text_file(
    value: Value,
    established_external_session: &str,
    expected: &ExpectedOwnerScope,
    owners: &impl DeferredReadTextOwnerResolver,
    service: &ReadTextFileService,
) -> Result<DeferredReadTextFile, &'static str> {
    let handoff = decode_read_text_file_value(value)?;
    if handoff.session_id != established_external_session {
        return Err("foreign read-text session");
    }
    let owner = owners
        .owner_for_external_session(established_external_session, expected)
        .filter(|owner| owner == &expected.owner)
        .ok_or("foreign read-text session")?;
    // `_meta` intentionally has no authority or routing meaning.
    let _meta = handoff.meta;
    let begin = service
        .begin_absolute(owner, Path::new(&handoff.absolute_path), handoff.range)
        .map_err(|_| "read-text resource unavailable")?;
    Ok(DeferredReadTextFile {
        request_id: handoff.request_id,
        begin,
    })
}

fn is_native_absolute(path: &str) -> bool {
    Path::new(path).is_absolute()
        || (path.len() >= 3
            && path.as_bytes()[0].is_ascii_alphabetic()
            && path.as_bytes()[1] == b':'
            && matches!(path.as_bytes()[2], b'\\' | b'/'))
        || path.starts_with(r"\\")
}

/// Strict standalone parser for fixtures. Production adapter ingress must call
/// its own wire decoder first, then `decode_read_text_file_value`.
#[cfg(test)]
pub(crate) fn decode_read_text_file_handoff(
    raw: &[u8],
) -> Result<ReadTextFileHandoff, &'static str> {
    decode_read_text_file_value(serde_json::from_slice(raw).map_err(|_| "malformed request")?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadTextFileResult {
    pub(crate) resource: ResourceRef,
    pub(crate) content: String,
    pub(crate) source_bytes: usize,
}

pub(crate) fn read_disk_text_file(
    workspace: &Workspace,
    resource: &ResourceRef,
    range: ReadTextRange,
    admission: Option<&NativeResourceAdmission>,
) -> AppResult<ReadTextFileResult> {
    // P-02 never canonicalizes a proof-carrying leaf and then reopens it. The
    // held-component walk compares the final native handle before reading it.
    let source = match admission {
        Some(admission) => {
            workspace.read_text_file_with_identity(resource, admission.file_identity())?
        }
        // Direct begin is a cfg(test) compatibility fixture, not production
        // ingress. Keep ordinary workspace behavior only for that seam.
        None => workspace.read_text_file(resource)?,
    };
    debug_assert!(source.content.len() <= P02_READ_TEXT_SOURCE_BYTES);
    let content = select_text_lines(&source.content, range.line, range.limit)?;
    Ok(ReadTextFileResult {
        resource: resource.clone(),
        content,
        source_bytes: source.byte_len as usize,
    })
}

#[allow(dead_code, reason = "P-02 sealed egress encoder is adapter-deferred")]
#[derive(Serialize)]
struct WireReadResult<'a> {
    content: &'a str,
}
#[allow(dead_code, reason = "P-02 sealed egress encoder is adapter-deferred")]
#[derive(Serialize)]
struct WireResponse<'a> {
    jsonrpc: &'static str,
    id: Value,
    result: WireReadResult<'a>,
}

/// Builds the only exceptional ACP response frame. This stays crate-private
/// to the completion lease: adapters may not manufacture an expanded response
/// without the live P-02 reservation that owns its source content.
#[allow(dead_code, reason = "P-02 sealed egress encoder is adapter-deferred")]
pub(crate) fn encode_read_text_file_response(
    request_id: &ReadTextRequestId,
    content: &str,
) -> Result<OutboundFrame, FrameWriteError> {
    if content.len() > P02_READ_TEXT_SOURCE_BYTES {
        return Err(FrameWriteError::Oversize);
    }
    let bytes = serde_json::to_vec(&WireResponse {
        jsonrpc: "2.0",
        id: request_id.as_value(),
        result: WireReadResult { content },
    })
    .map_err(|_| FrameWriteError::InvalidUtf8)?;
    OutboundFrame::new_p02_read_text_file_response(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inbound_handoff_is_strict_and_preserves_string_or_numeric_id() {
        let fixture_path = if cfg!(windows) {
            "C:/fixture/file.txt"
        } else {
            "/fixture/file.txt"
        };
        for (id, expected) in [("\"request-1\"", "\"request-1\""), ("42", "42")] {
            let raw = format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"fs/read_text_file\",\"params\":{{\"sessionId\":\"fixture-session\",\"path\":\"{fixture_path}\",\"line\":2,\"limit\":1}}}}"
            );
            let handoff = decode_read_text_file_handoff(raw.as_bytes()).unwrap();
            assert_eq!(handoff.absolute_path, fixture_path);
            let response = encode_read_text_file_response(&handoff.request_id, "text").unwrap();
            assert!(
                String::from_utf8(response.payload().to_vec())
                    .unwrap()
                    .contains(&format!("\"id\":{expected}"))
            );
        }
        for invalid in [
            format!(
                r#"{{"jsonrpc":"2.0","id":1.5,"method":"fs/read_text_file","params":{{"sessionId":"s","path":"{fixture_path}"}}}}"#
            ),
            format!(
                r#"{{"jsonrpc":"2.0","id":9223372036854775808,"method":"fs/read_text_file","params":{{"sessionId":"s","path":"{fixture_path}"}}}}"#
            ),
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"fs/read_text_file","params":{{"sessionId":"s","path":"{fixture_path}","_meta":true}}}}"#
            ),
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"fs/read_text_file","params":{{"sessionId":"s","path":"{fixture_path}","extra":true}}}}"#
            ),
        ] {
            assert!(decode_read_text_file_handoff(invalid.as_bytes()).is_err());
        }
    }
}
