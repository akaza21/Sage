use super::Mode;

pub(crate) const MAX_VALUE_BYTES: usize = 8192;
pub(crate) const MAX_VALUE_NEWLINES: usize = 64;
pub(crate) const DIAGNOSTIC_BYTES: usize = 1024;

pub(crate) fn byte_limit(mode: Mode) -> usize {
    if mode == Mode::Diagnostic {
        DIAGNOSTIC_BYTES
    } else {
        MAX_VALUE_BYTES
    }
}

pub(crate) fn quote_end(
    value: &str,
    mut cursor: usize,
    quote: u8,
    escaped_quote: bool,
    limit: usize,
    mode: Mode,
) -> Option<usize> {
    let bytes = value.as_bytes();
    let limit = limit.min(bytes.len());
    let mut escaped = false;
    let mut newlines = 0;
    while cursor < limit {
        newlines += usize::from(bytes[cursor] == b'\n');
        if mode != Mode::Diagnostic && newlines >= MAX_VALUE_NEWLINES {
            return None;
        }
        if escaped_quote && bytes[cursor] == b'\\' {
            let start = cursor;
            while cursor < limit && bytes[cursor] == b'\\' {
                cursor += 1;
            }
            if cursor < limit && bytes[cursor] == quote {
                cursor += 1;
                if (cursor - start - 1) % 4 == 1 {
                    return Some(cursor);
                }
            }
            continue;
        }
        let byte = bytes[cursor];
        cursor += 1;
        if escaped {
            escaped = false;
        } else if !escaped_quote && byte == quote {
            if quote != b'\''
                || !value[cursor..]
                    .chars()
                    .next()
                    .is_some_and(char::is_alphanumeric)
            {
                return Some(cursor);
            }
        } else if !escaped_quote && byte == b'\\' && quote != b'\'' {
            escaped = true;
        }
    }
    None
}

pub(crate) fn container_end(value: &str, start: usize, mode: Mode) -> Option<usize> {
    let bytes = value.as_bytes();
    let limit = start.saturating_add(byte_limit(mode)).min(bytes.len());
    let mut cursor = start;
    let mut depth = 0usize;
    let mut newlines = 0;
    while cursor < limit {
        newlines += usize::from(bytes[cursor] == b'\n');
        if mode != Mode::Diagnostic && newlines >= MAX_VALUE_NEWLINES {
            return None;
        }
        let span_start = cursor;
        match bytes[cursor] {
            b'"' | b'\'' => {
                cursor = quote_end(value, cursor + 1, bytes[cursor], false, limit, mode)? - 1
            }
            b'\\' if bytes.get(cursor + 1) == Some(&b'"') => {
                cursor = quote_end(value, cursor + 2, b'"', true, limit, mode)? - 1
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(cursor + 1);
                }
            }
            _ => {}
        }
        if cursor > span_start {
            newlines += bytes[span_start..=cursor]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count();
            if mode != Mode::Diagnostic && newlines >= MAX_VALUE_NEWLINES {
                return None;
            }
        }
        cursor += 1;
    }
    None
}
