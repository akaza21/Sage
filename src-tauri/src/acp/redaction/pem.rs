use std::borrow::Cow;

pub(crate) const QUOTE_SCAN_LIMIT: usize = 1024;

pub(crate) fn scan_window(value: &str) -> &str {
    let mut end = value.len().min(QUOTE_SCAN_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(crate) fn redact_pem(value: &str) -> Cow<'_, str> {
    redact_pem_with_status(value).text
}

pub(crate) struct RedactedPem<'a> {
    pub(crate) text: Cow<'a, str>,
    pub(crate) truncated: bool,
}

pub(crate) fn redact_pem_with_status(value: &str) -> RedactedPem<'_> {
    let Some((mut start, mut label)) = private_key_begin(value) else {
        return RedactedPem {
            text: Cow::Borrowed(value),
            truncated: false,
        };
    };
    let mut remaining = value;
    let mut output = String::with_capacity(value.len());
    let mut truncated = false;
    loop {
        output.push_str(&remaining[..start]);
        output.push_str("[redacted]");
        let block = &remaining[start..];
        let end = label.and_then(|label| {
            let end = format!("-----END {label}-----");
            scan_window(block).find(&end).map(|index| index + end.len())
        });
        let consumed = end.unwrap_or_else(|| {
            truncated = true;
            block.len()
        });
        remaining = &block[consumed..];
        let Some((next, next_label)) = private_key_begin(remaining) else {
            output.push_str(remaining);
            break;
        };
        start = next;
        label = next_label;
    }
    RedactedPem {
        text: Cow::Owned(output),
        truncated,
    }
}

fn private_key_begin(value: &str) -> Option<(usize, Option<&str>)> {
    const BEGIN: &str = "-----BEGIN ";
    value.match_indices(BEGIN).find_map(|(start, _)| {
        let remaining = &value[start + BEGIN.len()..];
        let header = scan_window(remaining);
        let Some(end) = header.find("-----") else {
            return (header.len() < remaining.len()).then_some((start, None));
        };
        let label = &header[..end];
        (label.ends_with("PRIVATE KEY") && !label.contains(['\r', '\n']))
            .then_some((start, Some(label)))
    })
}
