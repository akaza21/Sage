//! One bounded display line from a harness status message.

use super::super::support::sanitize;

const ELLIPSIS: char = '\u{2026}';

/// Collapses whitespace to single spaces, redacts secrets and paths, and
/// clips the result to `max` bytes with an ellipsis. Only a bounded prefix
/// of `raw` is read, so an oversized message costs no more than a short one.
/// Blank text yields `None`.
pub(super) fn status_line(raw: &str, max: usize) -> Option<String> {
    let prefix = clip(raw, max.saturating_mul(4));
    let collapsed = prefix.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = sanitize(&collapsed);
    if line.is_empty() {
        return None;
    }
    if line.len() <= max && prefix.len() == raw.len() {
        return Some(line);
    }
    let mut clipped = clip(&line, max.saturating_sub(ELLIPSIS.len_utf8())).to_owned();
    clipped.push(ELLIPSIS);
    Some(clipped)
}

fn clip(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::status_line;

    #[test]
    fn a_message_becomes_one_redacted_line_within_the_cap() {
        assert_eq!(
            status_line("  Fetched\n page 2   of 5 ", 512).as_deref(),
            Some("Fetched page 2 of 5")
        );
        assert_eq!(status_line(" \n\t", 512), None);
        assert_eq!(
            status_line("token sk-secret read", 512).as_deref(),
            Some("token [redacted] read")
        );
        let long = "\u{e9}".repeat(4096);
        let line = status_line(&long, 512).unwrap();
        assert!(line.len() <= 512);
        assert!(line.ends_with('\u{2026}'));
    }
}
