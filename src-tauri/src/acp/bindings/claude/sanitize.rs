use crate::acp::contract::{ContractError, SafeText};

const REDACTED: &str = "[redacted]";

pub(super) fn normalized_text<const MAX: usize>(
    field: &'static str,
    input: &str,
) -> Result<SafeText<MAX>, ContractError> {
    if input.len() > MAX {
        return SafeText::new(field, input);
    }
    let mut output = String::with_capacity(input.len().min(MAX));
    for (index, token) in input.split_inclusive(char::is_whitespace).enumerate() {
        let (body, whitespace) = split_trailing_whitespace(token);
        let previous = if index == 0 {
            None
        } else {
            output.split_whitespace().last()
        };
        if is_secret(body) || previous == Some("Bearer") || is_absolute_native_path(body) {
            output.push_str(REDACTED);
        } else {
            output.push_str(body);
        }
        output.push_str(whitespace);
    }
    SafeText::new(field, output)
}

fn split_trailing_whitespace(value: &str) -> (&str, &str) {
    let split = value
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(value.len());
    value.split_at(split)
}

fn is_secret(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    trimmed.starts_with("sk-ant-")
        || trimmed.starts_with("sk-")
        || trimmed.starts_with("Bearer:")
        || trimmed.starts_with("token=")
        || trimmed.starts_with("api_key=")
        || trimmed.starts_with("apiKey=")
}

fn is_absolute_native_path(value: &str) -> bool {
    let trimmed = value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | ':'
        )
    });
    trimmed.starts_with('/')
        || trimmed.starts_with("\\\\")
        || (trimmed.as_bytes().get(1) == Some(&b':')
            && trimmed
                .as_bytes()
                .get(2)
                .is_some_and(|separator| matches!(separator, b'\\' | b'/'))
            && trimmed
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_and_native_path_tokens() {
        let value = normalized_text::<512>(
            "fixture",
            r"result sk-ant-secret C:\private\file.txt /private/file Bearer abc123 safe",
        )
        .unwrap();
        assert_eq!(
            value.as_str(),
            "result [redacted] [redacted] [redacted] Bearer [redacted] safe"
        );
    }
}
