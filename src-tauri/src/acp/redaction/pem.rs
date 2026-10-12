use std::borrow::Cow;

const QUOTE_SCAN_LIMIT: usize = 1024;

pub(crate) fn scan_window(value: &str) -> &str {
    let mut end = value.len().min(QUOTE_SCAN_LIMIT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(crate) fn redact_pem(value: &str) -> Cow<'_, str> {
    let Some((mut start, mut label)) = private_key_begin(value) else {
        return Cow::Borrowed(value);
    };
    let mut remaining = value;
    let mut output = String::with_capacity(value.len());
    loop {
        output.push_str(&remaining[..start]);
        output.push_str("[redacted]");
        let block = &remaining[start..];
        let consumed = label.map_or(block.len(), |label| {
            let end = format!("-----END {label}-----");
            scan_window(block)
                .find(&end)
                .map_or(block.len(), |index| index + end.len())
        });
        remaining = &block[consumed..];
        let Some((next, next_label)) = private_key_begin(remaining) else {
            output.push_str(remaining);
            break;
        };
        start = next;
        label = next_label;
    }
    Cow::Owned(output)
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
