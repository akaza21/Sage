pub(super) fn value_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut index = start;
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        if matches!(bytes[index], b',' | b';' | b'}' | b']') {
            let next = index + 1;
            if next == bytes.len()
                || bytes[next].is_ascii_whitespace()
                || pair_start(&input[next..])
            {
                break;
            }
        }
        index += 1;
    }
    index
}

fn pair_start(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'"' | b'\'' | b'\\'))
    {
        index += 1;
    }
    let start = index;
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        index += 1;
    }
    while bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'"' | b'\'' | b'\\'))
    {
        index += 1;
    }
    index > start && matches!(bytes.get(index), Some(b':' | b'='))
}
