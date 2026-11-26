use unicode_segmentation::UnicodeSegmentation;

const PREFIX_BYTES: usize = 4096;
const TITLE_CHARS: usize = 50;
const TITLE_WORDS: usize = 6;
const FILLERS: &[&str] = &[
    "please",
    "can you",
    "could you",
    "help me",
    "I want you to",
    "let's",
];
const CONNECTORS: &[&str] = &[
    "and", "or", "to", "for", "with", "in", "on", "at", "of", "from", "by", "a", "an", "the",
    "before",
];

fn trailing_punctuation(value: char) -> bool {
    matches!(value, '?' | '!' | '.' | ',' | ':' | ';' | '…')
}

/// A bounded, deterministic summary of the first nonblank prompt line. Harness
/// and user titles keep their own sanitization and length rules.
pub(in crate::chat_lifecycle) fn prompt_title(text: &str) -> Option<String> {
    let mut end = text.len().min(PREFIX_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let (line, prefix_truncated) =
        text[..end]
            .split_inclusive(super::is_line_break)
            .find_map(|segment| {
                super::title_line(segment, PREFIX_BYTES).map(|line| {
                    (
                        line,
                        end < text.len() && !segment.ends_with(super::is_line_break),
                    )
                })
            })?;
    let normalized = line
        .split_whitespace()
        .fold(String::new(), |mut text, word| {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(word);
            text
        });
    let mut content = normalized.trim_end_matches(trailing_punctuation);
    let mut stripped = false;
    while let Some(rest) = FILLERS.iter().find_map(|filler| {
        let prefix = content.get(..filler.len())?;
        let rest = &content[filler.len()..];
        (prefix.eq_ignore_ascii_case(filler) && (rest.is_empty() || rest.starts_with(' ')))
            .then(|| rest.trim_start())
    }) {
        content = rest;
        stripped = true;
    }
    // Restore sentence casing after removing a conversational prefix, keeping
    // mixed-case names and identifier tokens intact.
    let first = content.split_whitespace().next()?;
    let content = if stripped && first.chars().all(char::is_lowercase) {
        let mut characters = content.chars();
        format!(
            "{}{}",
            characters.next()?.to_uppercase(),
            characters.as_str()
        )
    } else {
        content.to_owned()
    };
    let mut title = String::new();
    let mut characters = 0;
    let mut truncated = prefix_truncated;
    for (index, word) in content.split_whitespace().enumerate() {
        if index == TITLE_WORDS {
            truncated = true;
            break;
        }
        let separator = usize::from(!title.is_empty());
        if characters + separator + word.chars().count() <= TITLE_CHARS
            && title.len() + separator + word.len() <= super::super::MAX_CHAT_TITLE_BYTES
        {
            if separator != 0 {
                title.push(' ');
            }
            title.push_str(word);
            characters += separator + word.chars().count();
        } else {
            truncated = true;
            // Only an oversized first token is clipped: retain useful long
            // identifiers without leaving a fragment of a later word.
            if title.is_empty() {
                for grapheme in word.graphemes(true) {
                    let count = grapheme.chars().count();
                    if characters + count > TITLE_CHARS
                        || title.len() + grapheme.len() > super::super::MAX_CHAT_TITLE_BYTES
                    {
                        break;
                    }
                    title.push_str(grapheme);
                    characters += count;
                }
            }
            break;
        }
    }
    // A cut title ends at its last whole sentence of three or more words, so
    // it never stops mid-phrase after a complete request. A period after a
    // short or dotted token ("vs.", "e.g.") is not a sentence end.
    let cut = title
        .match_indices(' ')
        .filter_map(|(space, _)| {
            let sentence = &title[..space];
            let kept = sentence.strip_suffix(['.', '?', '!'])?;
            let token = kept.split_whitespace().next_back()?;
            let abbreviation = sentence.ends_with('.')
                && (token.contains('.') || token.chars().count() <= 2);
            (!abbreviation && kept.split_whitespace().nth(2).is_some())
                .then_some(kept.len())
        })
        .next_back()
        .filter(|_| truncated);
    if let Some(end) = cut {
        title.truncate(end);
    }
    let sentence = cut.is_some();
    let mut title = title.trim_end_matches(trailing_punctuation);
    while truncated
        && !sentence
        && let Some(last) = title.split_whitespace().next_back()
        && CONNECTORS
            .iter()
            .any(|connector| last.eq_ignore_ascii_case(connector))
    {
        title = title[..title.len() - last.len()].trim_end();
    }
    super::super::canonical_chat_title(title).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_conversational_prefixes_without_losing_the_request() {
        for (prompt, expected) in [
            (
                "Can you please check Sage Claude progress?",
                "Chat: Check Sage Claude progress",
            ),
            (
                "Please help me diagnose the recurring laptop pop-up…",
                "Chat: Diagnose the recurring laptop pop-up",
            ),
            (
                "\n \t\nCould you fix Sage issue #443?\nDetails",
                "Chat: Fix Sage issue #443",
            ),
            (
                "I want you to help me check iOS ACP_v2 #443",
                "Chat: Check iOS ACP_v2 #443",
            ),
            ("Let's inspect foo::bar and", "Chat: Inspect foo::bar and"),
            ("Turn logging on", "Chat: Turn logging on"),
            ("Explain bitwise OR", "Chat: Explain bitwise OR"),
            (
                "Check what happened before",
                "Chat: Check what happened before",
            ),
        ] {
            assert_eq!(
                prompt_title(prompt).as_deref(),
                Some(expected),
                "{prompt:?}"
            );
        }
    }

    #[test]
    fn preserves_names_identifiers_unicode_and_filtered_text() {
        for (prompt, expected) in [
            ("Please iOS ACP_v2 #443", "Chat: iOS ACP_v2 #443"),
            ("请检查 Sage 进度？", "Chat: 请检查 Sage 进度？"),
            (
                "Please écrire un résumé Sage",
                "Chat: Écrire un résumé Sage",
            ),
            (
                "Can\u{202e} you please fix\u{7} Sage\u{200b}\tnow?",
                "Chat: Fix Sage now",
            ),
            (
                "\u{feff}\n\u{2028}please check Sage\rLater",
                "Chat: Check Sage",
            ),
            (
                "Pleaseish is an identifier",
                "Chat: Pleaseish is an identifier",
            ),
            (
                "Investigate request_0123456789abcdef failure",
                "Chat: Investigate request_0123456789abcdef failure",
            ),
        ] {
            assert_eq!(
                prompt_title(prompt).as_deref(),
                Some(expected),
                "{prompt:?}"
            );
        }
        for prompt in [
            "",
            " \n\t",
            "Please help me",
            "Can you please?!",
            "let's",
            "I want you to",
            "\u{7}\u{202e}",
        ] {
            assert_eq!(prompt_title(prompt), None, "{prompt:?}");
        }
    }

    #[test]
    fn bounds_prefix_words_characters_and_utf8_bytes() {
        assert_eq!(
            prompt_title("Fix Sage ACP title fallback handling today"),
            Some("Chat: Fix Sage ACP title fallback handling".into())
        );
        assert_eq!(
            prompt_title("Fix Sage ACP titles before the next release"),
            Some("Chat: Fix Sage ACP titles".into())
        );
        assert_eq!(
            prompt_title("Investigate recurring laptop notifications with logs and traces"),
            Some("Chat: Investigate recurring laptop notifications".into())
        );
        let long_identifier = format!("request_{}", "x".repeat(100));
        assert_eq!(
            prompt_title(&long_identifier),
            Some(format!("Chat: request_{}", "x".repeat(42)))
        );
        assert_eq!(
            prompt_title(&"é".repeat(100)),
            Some(format!("Chat: {}", "é".repeat(50)))
        );
        assert_eq!(
            prompt_title(&"🙂".repeat(100)),
            Some(format!("Chat: {}", "🙂".repeat(32)))
        );
        assert_eq!(
            prompt_title(&"e\u{301}".repeat(30)),
            Some(format!("Chat: {}", "e\u{301}".repeat(25)))
        );
        assert_eq!(
            prompt_title("Work in this repository. Use your tools to run commands"),
            Some("Chat: Work in this repository".into())
        );
        assert_eq!(
            prompt_title("Why does login fail? Check the session store and logs"),
            Some("Chat: Why does login fail".into())
        );
        assert_eq!(
            prompt_title("Hi. Please fix the sidebar crash when it reloads"),
            Some("Chat: Hi. Please fix the sidebar crash".into())
        );
        assert_eq!(
            prompt_title("Hey Claude. Fix the login redirect bug today"),
            Some("Chat: Hey Claude. Fix the login redirect".into())
        );
        assert_eq!(
            prompt_title("Compare Vite vs. webpack and e.g. rollup configs"),
            Some("Chat: Compare Vite vs. webpack and e.g".into())
        );
        assert_eq!(
            prompt_title("What is this for? Explain the module layout"),
            Some("Chat: What is this for".into())
        );
        assert_eq!(
            prompt_title("Fix login. Then add tests"),
            Some("Chat: Fix login. Then add tests".into())
        );
        assert_eq!(prompt_title(&format!("{}Fix Sage", " ".repeat(4096))), None);
        assert_eq!(
            prompt_title(&format!("\n{}Fix Sage with logs", " ".repeat(4082))),
            Some("Chat: Fix Sage".into())
        );
        assert_eq!(
            prompt_title(&format!("\nTurn logging on\n{}", "x".repeat(4096))),
            Some("Chat: Turn logging on".into())
        );
        assert_eq!(
            prompt_title(&format!("{}é\nFix Sage", " ".repeat(4095))),
            None
        );
    }
}
