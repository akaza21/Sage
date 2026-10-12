use super::{
    QUOTE_SCAN_LIMIT, assignment_start, diagnostic_word, sanitize_words, sensitive_key, word_len,
};
use crate::acp::redaction::text::credentials::decode_key;

#[derive(PartialEq, Eq)]
enum SpanKind {
    Key,
    TextValue,
    EncodedValue,
}

pub(super) fn sanitize_word(
    value: &str,
    token_len: usize,
    json_depth: u8,
) -> Option<(String, usize)> {
    let bytes = value.as_bytes();
    let mut limit = token_len;
    let mut cursor = usize::from(value.starts_with("\"{") || value.starts_with("\"["));
    let mut copied = 0;
    let mut found_key = false;
    let mut output = String::new();
    while cursor < limit {
        if bytes[cursor] != b'"' {
            if found_key
                && matches!(bytes[cursor - 1], b',' | b'}' | b']')
                && assignment_boundary(&value[cursor..])
            {
                output.push_str(&value[copied..cursor]);
                return Some((output, cursor));
            }
            cursor += 1;
            continue;
        }
        let escaped = cursor > 0 && bytes[cursor - 1] == b'\\';
        let start = cursor - usize::from(escaped);
        let Some(end) = quote_end(value, cursor + 1, escaped, limit) else {
            break;
        };
        let key_end = end - 1 - usize::from(escaped);
        let key = &value[cursor + 1..key_end];
        cursor = end;
        let colon =
            end + value[end..].len() - value[end..].trim_start_matches(char::is_whitespace).len();
        if found_key && bytes.get(colon) == Some(&b'=') {
            output.push_str(&value[copied..start]);
            return Some((output, start));
        }
        if bytes.get(colon) != Some(&b':') {
            continue;
        }
        found_key = true;
        let decoded_key = decode_key(key, escaped);
        let sensitive = sensitive_key(&decoded_key);
        if let Some(sanitized) =
            sanitize_span(value, start, end, escaped, json_depth, SpanKind::Key)
        {
            output.push_str(&value[copied..start]);
            output.push_str(&sanitized);
            copied = end;
            found_key = true;
        }
        let colon_end = colon + 1;
        let secret_start = colon_end + value[colon_end..].len()
            - value[colon_end..]
                .trim_start_matches(char::is_whitespace)
                .len();
        let secret = &value[secret_start..];
        let quoted_start = if secret.starts_with("\\\"") {
            Some((secret_start + 2, true))
        } else if secret.starts_with('"') {
            Some((secret_start + 1, false))
        } else {
            None
        };
        if !sensitive {
            if let Some((content_start, escaped)) = quoted_start {
                let bounded_end = quote_end(
                    value,
                    content_start,
                    escaped,
                    (content_start + QUOTE_SCAN_LIMIT).min(value.len()),
                );
                if let Some(end) = quote_end(value, content_start, escaped, value.len()) {
                    output.push_str(&value[copied..secret_start]);
                    if let Some(sanitized) = sanitize_span(
                        value,
                        secret_start,
                        end,
                        escaped,
                        if bounded_end.is_some() { json_depth } else { 1 },
                        if encoded_payload_key(&decoded_key) {
                            SpanKind::EncodedValue
                        } else {
                            SpanKind::TextValue
                        },
                    ) {
                        output.push_str(&sanitized);
                    } else {
                        output.push_str(&value[secret_start..end]);
                    }
                    copied = end;
                    cursor = end;
                    found_key = true;
                    if cursor > limit {
                        limit = cursor + word_len(&value[cursor..]);
                    }
                } else {
                    break;
                }
            }
            continue;
        }
        found_key = true;
        let secret_end = value_end(value, secret_start);
        if secret_start == secret_end || secret.starts_with("[redacted]") {
            cursor = secret_end;
            continue;
        }
        output.push_str(&value[copied..secret_start]);
        output.push_str("[redacted]");
        copied = secret_end;
        cursor = secret_end;
        if cursor > limit {
            limit = cursor + word_len(&value[cursor..]);
        }
    }
    if !found_key {
        return None;
    }
    let tail = &value[copied..limit];
    let trimmed = tail.trim_start_matches([',', '}', ']']);
    output.push_str(&tail[..tail.len() - trimmed.len()]);
    output.push_str(&sanitize_words(trimmed, 0));
    Some((output, limit))
}

fn assignment_boundary(value: &str) -> bool {
    let token_len = value
        .find(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '{' | '}' | '"')
        })
        .unwrap_or(value.len());
    let token = &value[..token_len];
    let (prefix, bare, suffix) = diagnostic_word(token);
    assignment_start(token, prefix, bare, suffix).is_some() || sensitive_key(bare)
}

pub(super) fn scalar_tail(value: &str, token_len: usize) -> Option<usize> {
    value[..token_len]
        .match_indices(',')
        .find_map(|(index, _)| {
            let tail = &value[index + 1..];
            (tail.starts_with(['{', '[']) || starts_with_pair(tail)).then_some(index)
        })
}

fn starts_with_pair(value: &str) -> bool {
    value.starts_with('"')
        && quote_end(value, 1, false, (1 + QUOTE_SCAN_LIMIT).min(value.len())).is_some_and(|end| {
            value[end..]
                .trim_start_matches(char::is_whitespace)
                .starts_with(':')
        })
}

fn value_end(value: &str, start: usize) -> usize {
    let secret = &value[start..];
    let quoted_start = if secret.starts_with("\\\"") {
        Some((start + 2, true))
    } else if secret.starts_with('"') {
        Some((start + 1, false))
    } else {
        None
    };
    if let Some((content_start, escaped)) = quoted_start {
        quote_end(
            value,
            content_start,
            escaped,
            (content_start + QUOTE_SCAN_LIMIT).min(value.len()),
        )
        .unwrap_or(value.len())
    } else if secret.starts_with(['{', '[']) && !secret.starts_with("[redacted]") {
        container_end(value, start).unwrap_or(value.len())
    } else {
        start
            + secret
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, ',' | '}' | ']')
                })
                .unwrap_or(secret.len())
    }
}

fn sanitize_span(
    value: &str,
    start: usize,
    end: usize,
    escaped: bool,
    json_depth: u8,
    kind: SpanKind,
) -> Option<String> {
    let delimiter = 1 + usize::from(escaped);
    let span = &value[start + delimiter..end - delimiter];
    let decoded = decode_key(span, escaped);
    if kind != SpanKind::Key && base64_value(&decoded, kind) && !super::contains_token(&decoded) {
        return None;
    }
    let sanitized = sanitize_words(&decoded, json_depth - 1);
    (sanitized != decoded).then(|| {
        let mut encoded = sanitized.replace('\\', "\\\\").replace('"', "\\\"");
        if escaped {
            encoded = encoded.replace('\\', "\\\\");
        }
        format!(
            "{}{}{}",
            &value[start..start + delimiter],
            encoded,
            &value[end - delimiter..end]
        )
    })
}

fn encoded_payload_key(key: &str) -> bool {
    [
        "payload",
        "data",
        "base64",
        "image",
        "image/png",
        "image/jpeg",
        "image/jpg",
    ]
    .iter()
    .any(|candidate| key.eq_ignore_ascii_case(candidate))
}

fn base64_value(value: &str, kind: SpanKind) -> bool {
    let payload = value.trim_end_matches('=');
    let jpeg = kind == SpanKind::EncodedValue && jpeg_header(value);
    value.len() >= 16
        && value.len().is_multiple_of(4)
        && value.len() - payload.len() <= 2
        && (value.ends_with('=') || jpeg)
        && (jpeg
            || crate::acp::redaction::text::paths::absolute_path(
                value,
                crate::acp::redaction::text::Mode::Diagnostic,
            )
            .is_none())
        && payload
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

fn jpeg_header(value: &str) -> bool {
    if value.len() < 16 || !value.starts_with("/9j/") {
        return false;
    }
    let mut header = [0_u8; 12];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    for (chunk, bytes) in value.as_bytes()[..16].chunks_exact(4).enumerate() {
        let mut decoded = 0_u32;
        for byte in bytes {
            let Some(index) = alphabet.iter().position(|candidate| candidate == byte) else {
                return false;
            };
            decoded = (decoded << 6) | index as u32;
        }
        header[chunk * 3..chunk * 3 + 3].copy_from_slice(&decoded.to_be_bytes()[1..]);
    }
    let length = u16::from_be_bytes([header[4], header[5]]);
    match header[3] {
        0xe0 => length >= 16 && matches!(&header[6..11], b"JFIF\0" | b"JFXX\0"),
        0xe1 => length >= 8 && &header[6..12] == b"Exif\0\0",
        0xdb => length >= 67 && header[6] & 0x0f <= 3,
        0xc0..=0xcf | 0xdd | 0xe2..=0xef | 0xfe => length >= 2,
        _ => false,
    }
}

pub(super) fn container_end(value: &str, start: usize) -> Option<usize> {
    crate::acp::redaction::text::spans::container_end(
        value,
        start,
        crate::acp::redaction::text::Mode::Diagnostic,
    )
}

fn quote_end(value: &str, cursor: usize, escaped: bool, limit: usize) -> Option<usize> {
    crate::acp::redaction::text::spans::quote_end(
        value,
        cursor,
        b'"',
        escaped,
        limit,
        crate::acp::redaction::text::Mode::Diagnostic,
    )
}
