use std::collections::VecDeque;

use sage_core::limits::{LIMITS, RETAINED_TOOL_OUTPUT_LINES};

const TRUNCATED_PREFIX: &str = "[output truncated] ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetainedCommandOutput {
    pub(crate) text: String,
    pub(crate) truncated: bool,
}

/// Callers must sanitize the joined value before retaining it: separate block
/// sanitization would lose token context such as `Bearer` followed by a value.
pub(crate) fn join_command_output_blocks<'a>(
    blocks: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let maximum = LIMITS.wire.frame_bytes as usize;
    let mut joined = String::new();
    for block in blocks {
        let required = joined.len().checked_add(block.len())?;
        if required > maximum {
            return None;
        }
        joined.try_reserve(block.len()).ok()?;
        joined.push_str(block);
    }
    Some(joined)
}

/// Retains a UTF-8-safe suffix of already-sanitized command output.
pub(crate) fn retain_command_output(input: &str) -> Option<RetainedCommandOutput> {
    let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
    let mut lines = VecDeque::new();
    let mut bytes = 0_usize;
    let mut line_truncated = false;

    for line in input.split_inclusive('\n') {
        bytes = bytes.saturating_add(line.len());
        lines.push_back(line);
        if lines.len() > RETAINED_TOOL_OUTPUT_LINES
            && let Some(removed) = lines.pop_front()
        {
            bytes = bytes.saturating_sub(removed.len());
            line_truncated = true;
        }
    }

    let truncated = line_truncated || bytes > maximum;
    while bytes > maximum && lines.len() > 1 {
        if let Some(removed) = lines.pop_front() {
            bytes = bytes.saturating_sub(removed.len());
        }
    }

    let first_start = lines
        .front()
        .filter(|line| line.len() > maximum)
        .map(|line| suffix_start(line, maximum))
        .unwrap_or(0);
    let body_bytes = bytes.saturating_sub(first_start);
    let mut text = String::new();
    // `text` has exactly `body_bytes` bytes, so one fallible reservation makes
    // every following append allocation-free.
    text.try_reserve_exact(body_bytes).ok()?;
    for (index, line) in lines.into_iter().enumerate() {
        if index == 0 {
            text.push_str(&line[first_start..]);
        } else {
            text.push_str(line);
        }
    }

    Some(RetainedCommandOutput { text, truncated })
}

impl RetainedCommandOutput {
    pub(crate) fn display_text(&self) -> Option<String> {
        if !self.truncated {
            let mut display = String::new();
            display.try_reserve_exact(self.text.len()).ok()?;
            display.push_str(&self.text);
            return Some(display);
        }
        let maximum = LIMITS.wire.retained_tool_output_bytes as usize;
        let body_limit = maximum.checked_sub(TRUNCATED_PREFIX.len())?;
        let start = suffix_start(&self.text, body_limit);
        let body = &self.text[start..];
        let mut display = String::new();
        display
            .try_reserve_exact(TRUNCATED_PREFIX.len().checked_add(body.len())?)
            .ok()?;
        display.push_str(TRUNCATED_PREFIX);
        display.push_str(body);
        Some(display)
    }
}

fn suffix_start(value: &str, maximum: usize) -> usize {
    let target = value.len().saturating_sub(maximum);
    value
        .char_indices()
        .find_map(|(index, _)| (index >= target).then_some(index))
        .unwrap_or(value.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_an_exact_byte_limit_without_a_marker() {
        let input = "a".repeat(LIMITS.wire.retained_tool_output_bytes as usize);
        let output = retain_command_output(&input).expect("retained output");
        assert_eq!(output.text, input);
        assert_eq!(output.display_text().as_deref(), Some(input.as_str()));
        assert!(!output.truncated);
    }

    #[test]
    fn marks_and_bounds_a_plus_one_byte_value() {
        let input = "a".repeat(LIMITS.wire.retained_tool_output_bytes as usize + 1);
        let output = retain_command_output(&input).expect("retained output");
        assert!(!output.text.starts_with(TRUNCATED_PREFIX));
        assert!(output.truncated);
        assert_eq!(
            output.text.len(),
            LIMITS.wire.retained_tool_output_bytes as usize
        );
        let display = output.display_text().expect("bounded display");
        assert!(display.starts_with(TRUNCATED_PREFIX));
        assert!(display.len() <= LIMITS.wire.retained_tool_output_bytes as usize);
    }

    #[test]
    fn preserves_200_lines_and_displays_at_most_200_after_201() {
        let exact = (0..RETAINED_TOOL_OUTPUT_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let exact_output = retain_command_output(&exact).expect("retained output");
        assert_eq!(exact_output.text, exact);
        assert!(!exact_output.truncated);
        assert_eq!(
            exact_output.text.split_inclusive('\n').count(),
            RETAINED_TOOL_OUTPUT_LINES
        );

        let over = (0..=RETAINED_TOOL_OUTPUT_LINES)
            .map(|index| format!("line-{index}\n"))
            .collect::<String>();
        let over_output = retain_command_output(&over).expect("retained output");
        assert!(!over_output.text.starts_with(TRUNCATED_PREFIX));
        assert!(!over_output.text.contains("line-0\n"));
        assert!(over_output.text.contains("line-200\n"));
        let display = over_output.display_text().expect("bounded display");
        assert!(display.starts_with(TRUNCATED_PREFIX));
        assert_eq!(
            display.split_inclusive('\n').count(),
            RETAINED_TOOL_OUTPUT_LINES
        );
    }

    #[test]
    fn keeps_unicode_crlf_and_empty_output_valid() {
        let unicode = "😀".repeat(LIMITS.wire.retained_tool_output_bytes as usize);
        let unicode_output = retain_command_output(&unicode).expect("retained output");
        assert!(unicode_output.truncated);
        assert!(
            std::str::from_utf8(unicode_output.display_text().expect("display").as_bytes()).is_ok()
        );

        let crlf_output = retain_command_output("one\r\ntwo\r\n").expect("retained output");
        assert_eq!(crlf_output.text, "one\r\ntwo\r\n");
        assert!(!crlf_output.truncated);

        let empty_output = retain_command_output("").expect("retained output");
        assert_eq!(empty_output.text, "");
        assert!(!empty_output.truncated);
    }

    #[test]
    fn joins_multiple_blocks_without_inventing_a_separator() {
        let joined = join_command_output_blocks(["Bearer", " secret"]);
        assert_eq!(joined.as_deref(), Some("Bearer secret"));
    }
}
