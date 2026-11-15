//! A journey frame names `$PROMPT_ECHO` where the agent's reply should show
//! the last prompt it received, `$CODEX_TURN_CONFIG` for the model and
//! effort of the last Codex `turn/start`, or `$TRANSCRIPT_ECHO` for what a
//! Claude Code session held before its last prompt (see `quote`). Each content block
//! is described by its kind; text is shown as sent, up to a bound, so a
//! check in the real app can read exactly what reached the agent. Binary
//! data is described by its media type and size, never shown.

use serde_json::Value;

use super::super::FixtureError;
use super::WireState;

const SHOWN_BLOCKS: usize = 16;
const SHOWN_TEXT_BYTES: usize = 512;

type Sent = fn(&WireState) -> Option<&str>;

const QUOTES: [(&str, Sent); 3] = [
    ("$CODEX_TURN_CONFIG", |state| {
        state.codex_turn_config.as_deref()
    }),
    ("$PROMPT_ECHO", |state| state.prompt_echo.as_deref()),
    ("$TRANSCRIPT_ECHO", |state| {
        state.claude_sdk.transcript_echo()
    }),
];

/// `text` with each client placeholder replaced by what the client sent, in
/// one pass, so text the client sent is never read as a placeholder. A frame
/// that quotes something the client has not sent fails the case.
pub(super) fn quote(text: &str, state: &WireState) -> Result<String, FixtureError> {
    let mut quoted = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('$') {
        quoted.push_str(&rest[..start]);
        rest = &rest[start..];
        match QUOTES.iter().find(|(token, _)| rest.starts_with(token)) {
            Some((token, sent)) => {
                quoted.push_str(sent(state).ok_or(FixtureError::InvalidArguments)?);
                rest = &rest[token.len()..];
            }
            None => {
                quoted.push('$');
                rest = &rest[1..];
            }
        }
    }
    quoted.push_str(rest);
    Ok(quoted)
}

pub(super) fn blocks(blocks: &[Value]) -> String {
    let mut lines = vec![match blocks.len() {
        1 => "Synthetic replay received 1 prompt block:".to_owned(),
        count => format!("Synthetic replay received {count} prompt blocks:"),
    }];
    lines.extend(blocks.iter().take(SHOWN_BLOCKS).map(block));
    if blocks.len() > SHOWN_BLOCKS {
        lines.push(format!("And {} more blocks.", blocks.len() - SHOWN_BLOCKS));
    }
    lines.join("\n\n")
}

fn block(block: &Value) -> String {
    let field = |key: &str| block.get(key).and_then(Value::as_str);
    match field("type") {
        Some("text") => shown_text("Text", field("text").unwrap_or_default()),
        // Messages API images and documents carry a base64 source; ACP
        // images carry their data and type directly.
        Some(kind @ ("image" | "document")) => {
            let source = block.get("source").unwrap_or(block);
            let text = |key: &str| source.get(key).and_then(Value::as_str);
            let media = text("media_type").or_else(|| text("mimeType"));
            format!(
                "{} ({}, {} base64 bytes)",
                capitalized(kind),
                media.unwrap_or("no media type"),
                text("data").map_or(0, str::len)
            )
        }
        Some("resource") => {
            let resource = block.get("resource").unwrap_or(&Value::Null);
            let text = |key: &str| resource.get(key).and_then(Value::as_str);
            let uri = text("uri").unwrap_or("no URI");
            match (text("text"), text("blob")) {
                (Some(body), _) => format!("Embedded file {uri}, {} bytes of text", body.len()),
                (None, Some(blob)) => format!("Embedded file {uri}, {} base64 bytes", blob.len()),
                (None, None) => format!("Embedded file {uri}, no content"),
            }
        }
        Some("resource_link") => format!("Linked file {}", field("uri").unwrap_or("no URI")),
        Some("skill") => format!(
            "Skill {} at {}",
            field("name").unwrap_or("with no name"),
            field("path").unwrap_or("no path")
        ),
        Some(other) => format!("A {other} block"),
        None => "A block with no type".to_owned(),
    }
}

/// Text in a fence no backtick run inside it can close.
fn shown_text(label: &str, text: &str) -> String {
    let mut cut = text.len().min(SHOWN_TEXT_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let size = if cut < text.len() {
        format!(", {} bytes, first {cut} shown", text.len())
    } else {
        String::new()
    };
    let shown = &text[..cut];
    let longest = shown
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{label}{size}:\n{fence}text\n{shown}\n{fence}")
}

fn capitalized(kind: &str) -> String {
    let mut characters = kind.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn credential_notes_and_mentions_read_as_sent() {
        let echo = blocks(&[
            json!({"type":"text","text":"Check these"}),
            json!({"type":"text","text":"<context ref=\"file:///p/.env\">\n(Not attached: Sage does not send credential files.)\n</context>"}),
            json!({"type":"text","text":"@/p/big.log"}),
        ]);
        assert!(echo.starts_with("Synthetic replay received 3 prompt blocks:"));
        assert!(echo.contains("(Not attached: Sage does not send credential files.)"));
        assert!(echo.contains("```text\n@/p/big.log\n```"));
    }

    #[test]
    fn long_text_is_cut_on_a_character_and_binary_data_is_only_sized() {
        let long = "é".repeat(400);
        let echo = blocks(&[
            json!({"type":"text","text":long}),
            json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0K"}}),
            json!({"type":"resource","resource":{"uri":"file:///p/a.txt","text":"abc"}}),
            json!({"type":"resource_link","uri":"file:///p/b.txt","name":"b.txt"}),
        ]);
        assert!(echo.contains("Text, 800 bytes, first 512 shown:"));
        assert!(echo.contains("Image (image/png, 8 base64 bytes)"));
        assert!(echo.contains("Embedded file file:///p/a.txt, 3 bytes of text"));
        assert!(echo.contains("Linked file file:///p/b.txt"));
        assert!(!echo.contains("iVBORw0K"));
    }

    #[test]
    fn a_fence_outlasts_backticks_in_the_text() {
        let echo = blocks(&[json!({"type":"text","text":"run ```rm``` here"})]);
        assert!(echo.contains("````text\nrun ```rm``` here\n````"));
    }
}
