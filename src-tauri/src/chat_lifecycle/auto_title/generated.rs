/// Generated output is a single plain title, unlike a user/harness metadata
/// title which can intentionally contain punctuation or a longer phrase.
pub(crate) fn generated_title(raw: &str) -> Option<String> {
    if raw.len() > 1024 || raw.trim().contains(super::is_line_break) {
        return None;
    }
    let clean = super::title_line(raw.trim().trim_matches(['"', '\'', '`']), 1024)?;
    if clean.contains(['{', '}', '[', ']', '#', '*', '`']) {
        return None;
    }
    let clean = clean.strip_prefix("Title:").unwrap_or(&clean).trim();
    let mut title = String::new();
    for word in clean.split_whitespace().take(6) {
        if title.chars().count() + usize::from(!title.is_empty()) + word.chars().count() > 50
            || title.len() + usize::from(!title.is_empty()) + word.len()
                > super::super::MAX_CHAT_TITLE_BYTES
        {
            break;
        }
        if !title.is_empty() {
            title.push(' ')
        }
        title.push_str(word);
    }
    let title = title.trim_end_matches(['.', '!', '?', '…']);
    if title.split_whitespace().count() < 3 || super::super::title_source::is_placeholder(title) {
        return None;
    }
    Some(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_one_bounded_plain_title_and_rejects_malformed_or_generic_output() {
        assert_eq!(
            generated_title("\"Investigate empty sidebar chats\""),
            Some("Investigate empty sidebar chats".into())
        );
        assert_eq!(
            generated_title("Fix the sidebar and keep all chats visible"),
            Some("Fix the sidebar and keep all".into())
        );
        for raw in [
            "",
            "New conversation",
            "Untitled chat",
            "Fix sidebar",
            "First title\nSecond title",
            "{\"title\":\"Fix the sidebar\"}",
            "### Fix the sidebar",
        ] {
            assert_eq!(generated_title(raw), None, "{raw:?}");
        }
        assert_eq!(generated_title(&"x".repeat(1025)), None);
    }
}
