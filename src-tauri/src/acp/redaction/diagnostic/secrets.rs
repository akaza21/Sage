use std::borrow::Cow;

use super::QUOTE_SCAN_LIMIT;

pub(super) fn scan_window(value: &str) -> &str {
    let mut end = value.len().min(QUOTE_SCAN_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(super) fn redact_pem(value: &str) -> Cow<'_, str> {
    const BEGIN: &str = "-----BEGIN RSA PRIVATE KEY-----";
    const END: &str = "-----END RSA PRIVATE KEY-----";
    let Some(mut start) = value.find(BEGIN) else {
        return Cow::Borrowed(value);
    };
    let mut remaining = value;
    let mut output = String::with_capacity(value.len());
    loop {
        output.push_str(&remaining[..start]);
        output.push_str("[redacted]");
        let block = &remaining[start..];
        let consumed = scan_window(block)
            .find(END)
            .map_or(block.len(), |index| index + END.len());
        remaining = &block[consumed..];
        let Some(next) = remaining.find(BEGIN) else {
            output.push_str(remaining);
            break;
        };
        start = next;
    }
    Cow::Owned(output)
}

pub(super) fn contains_sk_secret(value: &str) -> bool {
    value.match_indices("sk-").any(|(index, _)| {
        !value[..index]
            .chars()
            .next_back()
            .is_some_and(char::is_alphabetic)
            && value[index + 3..]
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .take(3)
                .count()
                == 3
    })
}

pub(super) fn url_userinfo(token: &str) -> Option<(usize, usize)> {
    let start = token.find("://")? + 3;
    let authority = token[start..].split(['/', '?', '#']).next()?;
    let end = start + authority.rfind('@')?;
    Some((start, end))
}
