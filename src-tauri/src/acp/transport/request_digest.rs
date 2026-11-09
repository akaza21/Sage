use sha2::{Digest, Sha256};

use super::{Frame, StructuralError};

const DOMAIN: &[u8] = b"sage-acp-request-v1\0";

/// Hashes the exact top-level JSON-RPC request identity from a structurally
/// validated frame. The method is hashed as decoded UTF-8 while `params` keeps
/// its exact JSON token bytes; neither value is retained after this call.
pub(crate) fn request_payload_digest(frame: &Frame) -> Result<[u8; 32], StructuralError> {
    let (method, params) = request_parts(frame.as_bytes())?;
    Ok(digest_parts(method.as_deref(), params))
}

/// The same identity for a request Sage holds as a value, such as one a
/// binding derived from its harness's own wire frame.
pub(crate) fn request_value_digest(value: &serde_json::Value) -> [u8; 32] {
    let params = value
        .get("params")
        .and_then(|params| serde_json::to_vec(params).ok());
    digest_parts(
        value.get("method").and_then(serde_json::Value::as_str),
        params.as_deref(),
    )
}

fn digest_parts(method: Option<&str>, params: Option<&[u8]>) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(DOMAIN);
    update_part(&mut digest, method.map(str::as_bytes));
    update_part(&mut digest, params);
    digest.finalize().into()
}

fn update_part(digest: &mut Sha256, value: Option<&[u8]>) {
    let bytes = value.unwrap_or_default();
    digest.update([u8::from(value.is_some())]);
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

fn request_parts(bytes: &[u8]) -> Result<(Option<String>, Option<&[u8]>), StructuralError> {
    let mut cursor = skip_whitespace(bytes, 0);
    if bytes.get(cursor) != Some(&b'{') {
        return Err(StructuralError::RootType);
    }
    cursor += 1;
    let mut method = None;
    let mut params = None;
    loop {
        cursor = skip_whitespace(bytes, cursor);
        if bytes.get(cursor) == Some(&b'}') {
            return Ok((method, params));
        }
        let key_start = cursor;
        let key_end = string_end(bytes, key_start)?;
        let key: String = serde_json::from_slice(&bytes[key_start..key_end])
            .map_err(|_| StructuralError::Syntax)?;
        cursor = skip_whitespace(bytes, key_end);
        if bytes.get(cursor) != Some(&b':') {
            return Err(StructuralError::Syntax);
        }
        cursor = skip_whitespace(bytes, cursor + 1);
        let value_start = cursor;
        let value_end = value_end(bytes, value_start)?;
        match key.as_str() {
            "method" => {
                method = Some(
                    serde_json::from_slice(&bytes[value_start..value_end])
                        .map_err(|_| StructuralError::MethodType)?,
                );
            }
            "params" => params = Some(&bytes[value_start..value_end]),
            _ => {}
        }
        cursor = skip_whitespace(bytes, value_end);
        match bytes.get(cursor) {
            Some(b',') => cursor += 1,
            Some(b'}') => return Ok((method, params)),
            _ => return Err(StructuralError::Syntax),
        }
    }
}

fn value_end(bytes: &[u8], start: usize) -> Result<usize, StructuralError> {
    match bytes.get(start) {
        Some(b'"') => string_end(bytes, start),
        Some(b'{') | Some(b'[') => composite_end(bytes, start),
        Some(_) => {
            let mut cursor = start;
            while cursor < bytes.len()
                && !matches!(
                    bytes[cursor],
                    b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n'
                )
            {
                cursor += 1;
            }
            (cursor > start)
                .then_some(cursor)
                .ok_or(StructuralError::Syntax)
        }
        None => Err(StructuralError::Syntax),
    }
}

fn string_end(bytes: &[u8], start: usize) -> Result<usize, StructuralError> {
    if bytes.get(start) != Some(&b'"') {
        return Err(StructuralError::Syntax);
    }
    let mut cursor = start + 1;
    let mut escaped = false;
    while let Some(byte) = bytes.get(cursor) {
        cursor += 1;
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return Ok(cursor);
        }
    }
    Err(StructuralError::Syntax)
}

fn composite_end(bytes: &[u8], start: usize) -> Result<usize, StructuralError> {
    let mut cursor = start;
    let mut stack = Vec::with_capacity(32);
    let mut quoted = false;
    let mut escaped = false;
    while let Some(byte) = bytes.get(cursor).copied() {
        cursor += 1;
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' if stack.pop() == Some(byte) => {
                if stack.is_empty() {
                    return Ok(cursor);
                }
            }
            b'}' | b']' => return Err(StructuralError::Syntax),
            _ => {}
        }
    }
    Err(StructuralError::Syntax)
}

fn skip_whitespace(bytes: &[u8], mut cursor: usize) -> usize {
    while bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n'))
    {
        cursor += 1;
    }
    cursor
}
