//! What a replay commits: the commands a binding listed and one row per
//! event, with every id replaced by the order it was first seen and every
//! piece of prose by its length, so a wrong id, a lost Turn link or an
//! empty or duplicated reply changes the projection while the model's words
//! stay out of it.

use std::collections::HashMap;

use serde_json::{Value, json};

use super::super::adapter::{AdapterEvent, HarnessAdapter};

const IDS: [&str; 15] = [
    "id",
    "itemId",
    "item_id",
    "turnId",
    "turn_id",
    "externalRequestId",
    "external_request_id",
    "externalItemId",
    "external_item_id",
    "childThreadId",
    "child_thread_id",
    "childTurnId",
    "child_turn_id",
    "request_id",
    "external_id",
];
const PROSE: [&str; 7] = [
    "text",
    "summary",
    "inputSummary",
    "outputSummary",
    "old_text",
    "new_text",
    "details",
];
/// Bulk a row does not need: the payload digest restates the payload, and
/// the capability snapshot is the binding's static offer.
const DROPPED: [&str; 4] = [
    "requestPayloadDigest",
    "request_payload_digest",
    "capabilities",
    "observedSequence",
];

#[derive(Default)]
struct Ids(HashMap<String, usize>);

impl Ids {
    fn name(&mut self, value: &Value) -> Value {
        let next = self.0.len() + 1;
        json!(format!(
            "#{}",
            self.0.entry(value.to_string()).or_insert(next)
        ))
    }

    fn prune(&mut self, value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.retain(|key, _| !DROPPED.contains(&key.as_str()));
                for (key, entry) in map.iter_mut() {
                    if IDS.contains(&key.as_str()) && !entry.is_null() {
                        *entry = self.name(entry);
                    } else if PROSE.contains(&key.as_str()) && entry.is_string() {
                        let length = entry.as_str().map_or(0, |text| text.chars().count());
                        *entry = json!(format!("<{length} chars>"));
                    } else {
                        self.prune(entry);
                    }
                }
            }
            Value::Array(list) => list.iter_mut().for_each(|entry| self.prune(entry)),
            Value::String(text) if text.chars().count() > 48 => {
                let cut = text.char_indices().nth(48).map_or(text.len(), |(at, _)| at);
                text.truncate(cut);
                text.push('…');
            }
            _ => {}
        }
    }

    fn row(&mut self, event: &AdapterEvent) -> String {
        let mut value = serde_json::to_value(event).expect("event JSON");
        self.prune(&mut value);
        match value {
            Value::String(name) => name,
            Value::Object(map) => map
                .into_iter()
                .map(|(name, body)| format!("{name} {body}"))
                .collect::<Vec<_>>()
                .join(" "),
            other => other.to_string(),
        }
    }
}

pub(super) fn summary(
    adapter: &dyn HarnessAdapter,
    events: &[AdapterEvent],
    sections: Value,
) -> Value {
    let commands: Vec<String> = adapter
        .available_commands()
        .snapshot()
        .commands
        .iter()
        .map(|command| command.name.clone())
        .collect();
    let mut ids = Ids::default();
    let mut rows: Vec<(String, usize)> = Vec::new();
    for event in events {
        let row = ids.row(event);
        match rows.last_mut() {
            Some((last, count)) if *last == row => *count += 1,
            _ => rows.push((row, 1)),
        }
    }
    let mut summary = sections;
    summary["commands"] = json!(commands);
    summary["rows"] = json!(
        rows.into_iter()
            .map(|(row, count)| if count == 1 {
                row
            } else {
                format!("{row} x{count}")
            })
            .collect::<Vec<_>>()
    );
    summary
}
