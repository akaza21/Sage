//! What a permission card says about the tool it asks for, beyond the tool's
//! label: the text a mode switch carries (a plan to approve), and file
//! actions worded as the pending question they are.

use serde_json::{Map, Value};

use super::decode::fits_permission_payload;

/// The most characters of a mode switch's text a card shows.
const SWITCH_TEXT_CHARS: usize = 16_384;

/// The label followed by the tool call's text content, cut to fit the card;
/// the label alone when the call carries no text.
pub(super) fn with_text_content(label: &str, tool: &Map<String, Value>) -> String {
    let text = tool
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|block| block.get("content")?.get("text")?.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut limit = SWITCH_TEXT_CHARS;
    while limit > 0 && !text.is_empty() {
        let mut shown = format!("{label}\n\n");
        shown.extend(text.chars().take(limit));
        if text.chars().nth(limit).is_some() {
            shown.push('…');
        }
        if fits_permission_payload(&shown) {
            return shown;
        }
        limit /= 2;
    }
    label.to_owned()
}

/// A file action's label while it still waits for approval: "Edit a.rs?"
/// rather than the "Edited a.rs" its row reads once it ran.
pub(super) fn pending_file_action(label: &str) -> Option<String> {
    [("Edited ", "Edit "), ("Created ", "Create "), ("Deleted ", "Delete ")]
        .into_iter()
        .find_map(|(done, ask)| {
            label
                .strip_prefix(done)
                .map(|path| format!("{ask}{path}?"))
        })
}
