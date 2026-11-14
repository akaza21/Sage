//! Labels are built from the typed fields alone, sanitized, and never from the
//! raw command line.

use serde_json::{Map, Value};

use crate::acp::bindings::codex::incoming::support::sanitize;

const MAX_ACTIONS: usize = 8;
const PHRASE_CHARS: usize = 120;
const LABEL_CHARS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandExplore {
    /// Every action is a typed read. Carries the file name for a single read.
    Read(Option<String>),
    /// Searches and listings as imperative labels, in wire order.
    Search(String),
}

impl CommandExplore {
    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Read(name) => name.as_ref().map_or(0, String::len),
            Self::Search(label) => label.len(),
        }
    }
}

pub(crate) fn classify(actions: &Value) -> Option<CommandExplore> {
    let actions = actions.as_array()?;
    if actions.is_empty() || actions.len() > MAX_ACTIONS {
        return None;
    }
    let mut phrases = Vec::new();
    let mut name = None;
    for action in actions {
        let action = action.as_object()?;
        match action.get("type")?.as_str()? {
            "read" => name = action.get("name").and_then(Value::as_str).and_then(display),
            "search" => phrases.push(phrase(
                action,
                match action
                    .get("query")
                    .and_then(Value::as_str)
                    .and_then(display)
                {
                    Some(query) => format!("Search {query}"),
                    None => "Search files".to_owned(),
                },
            )),
            "listFiles" => phrases.push(phrase(action, "List files".to_owned())),
            _ => return None,
        }
    }
    if phrases.is_empty() {
        return Some(CommandExplore::Read(name.filter(|_| actions.len() == 1)));
    }
    let label: String = phrases.join(", ").chars().take(LABEL_CHARS).collect();
    Some(CommandExplore::Search(label))
}

/// Appends a workspace-relative `in {path}`; absolute or drive paths are
/// omitted rather than shown.
fn phrase(action: &Map<String, Value>, verb: String) -> String {
    let path = action
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.starts_with(['/', '\\', '~']) && !path.contains(':'))
        .and_then(display);
    match path {
        Some(path) => format!("{verb} in {path}"),
        None => verb,
    }
}

fn display(value: &str) -> Option<String> {
    if value.contains('\0') {
        return None;
    }
    let flat: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(PHRASE_CHARS)
        .collect();
    let shown = sanitize(flat.trim());
    let shown = shown.trim();
    (!shown.is_empty()).then(|| shown.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{CommandExplore, classify};

    #[test]
    fn typed_reads_classify_as_reads_with_the_single_file_name() {
        assert_eq!(
            classify(
                &json!([{"type":"read","command":"cat a","name":"package.json","path":"/w/package.json"}])
            ),
            Some(CommandExplore::Read(Some("package.json".into())))
        );
        assert_eq!(
            classify(&json!([
                {"type":"read","command":"cat a","name":"a.md","path":"/w/a.md"},
                {"type":"read","command":"cat b","name":"b.md","path":"/w/b.md"}
            ])),
            Some(CommandExplore::Read(None))
        );
    }

    #[test]
    fn searches_and_listings_build_imperative_labels_without_absolute_paths() {
        assert_eq!(
            classify(&json!([
                {"type":"search","command":"rg foo src","query":"foo","path":"src"},
                {"type":"listFiles","command":"ls","path":"/home/me/secret"}
            ])),
            Some(CommandExplore::Search(
                "Search foo in src, List files".into()
            ))
        );
        assert_eq!(
            classify(
                &json!([{"type":"search","command":"rg --files","query":null,"path":"C:\\w"}])
            ),
            Some(CommandExplore::Search("Search files".into()))
        );
    }

    #[test]
    fn unknown_or_malformed_actions_stay_ordinary_commands() {
        for actions in [
            json!([]),
            json!([{"type":"unknown","command":"node x.js"}]),
            json!([{"type":"search","query":"x"},{"type":"unknown","command":"y"}]),
            json!({"type":"read"}),
            Value::Array(vec![json!({"type": "read"}); 9]),
        ] {
            assert_eq!(classify(&actions), None, "{actions}");
        }
    }

    #[test]
    fn labels_are_flattened_and_sanitized() {
        let Some(CommandExplore::Search(label)) =
            classify(&json!([{"type":"search","command":"rg","query":"a\nsk-secret-token"}]))
        else {
            panic!("search");
        };
        assert!(
            !label.contains('\n') && !label.contains("sk-secret"),
            "{label}"
        );
    }
}
