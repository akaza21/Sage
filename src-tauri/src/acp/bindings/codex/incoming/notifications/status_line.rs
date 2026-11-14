use super::super::support::sanitize;
use crate::acp::redaction::text::{Mode, redact};

const ELLIPSIS: char = '\u{2026}';

/// Only a bounded prefix of `raw` is read, so an oversized message costs no
/// more than a short one.
pub(super) fn status_line(raw: &str, max: usize) -> Option<String> {
    let prefix = clip(raw, max.saturating_mul(4));
    let collapsed = prefix.split_whitespace().collect::<Vec<_>>().join(" ");
    let sanitized = sanitize(&collapsed);
    let bound = sanitized.len().saturating_mul(2).saturating_add(64);
    let line = redact("statusLine", &sanitized, bound, Mode::Prose).ok()?;
    if line.trim().is_empty() {
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
        let line = status_line("wrote /srv/app/secret.txt with key=abc123", 512).unwrap();
        assert!(line.starts_with("wrote "), "{line}");
        assert!(!line.contains("/srv/app"), "{line}");
        assert!(!line.contains("abc123"), "{line}");
        let long = "\u{e9}".repeat(4096);
        let line = status_line(&long, 512).unwrap();
        assert!(line.len() <= 512);
        assert!(line.ends_with('\u{2026}'));
    }
}
