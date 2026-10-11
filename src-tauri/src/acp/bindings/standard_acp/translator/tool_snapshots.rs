use serde_json::{Map, Value};

use crate::acp::{
    command_output::{join_command_output_blocks, retain_command_output},
    contract::{BoundedVec, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation, ToolCategory},
    limits::LIMITS,
};

use super::super::sanitize::{normalized_text, sanitized_command_output};
use super::super::surface::ToolCallContentKind;
use super::{ResourceMapper, StandardAcpTranslator, TranslationError};

type CommandSnapshot = (
    Option<SafeText<65536>>,
    BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
);

impl StandardAcpTranslator {
    pub(super) fn command_snapshot(&self, id: &str) -> CommandSnapshot {
        self.command_snapshots
            .get(id)
            .cloned()
            .unwrap_or((None, Default::default()))
    }

    pub(super) fn validate_command_snapshot(
        &self,
        id: &str,
        next: &CommandSnapshot,
    ) -> Result<(), TranslationError> {
        let old = self
            .command_snapshots
            .get(id)
            .map(command_snapshot_bytes)
            .unwrap_or(0);
        let total = self
            .command_snapshot_bytes
            .checked_sub(old)
            .and_then(|value| value.checked_add(command_snapshot_bytes(next)))
            .ok_or(TranslationError::ItemLimit)?;
        (total <= LIMITS.wire.item_bytes_per_turn as usize)
            .then_some(())
            .ok_or(TranslationError::ItemLimit)
    }

    pub(super) fn admit_snapshot(
        &self,
        id: &str,
        snapshot: CommandSnapshot,
        command: bool,
    ) -> Result<CommandSnapshot, TranslationError> {
        match self.validate_command_snapshot(id, &snapshot) {
            Ok(()) => Ok(snapshot),
            Err(error) if command => Err(error),
            Err(_) => Ok((None, Default::default())),
        }
    }

    pub(super) fn store_command_snapshot(&mut self, id: &str, next: CommandSnapshot) {
        let old = self
            .command_snapshots
            .get(id)
            .map(command_snapshot_bytes)
            .unwrap_or(0);
        self.command_snapshot_bytes = self
            .command_snapshot_bytes
            .saturating_sub(old)
            .saturating_add(command_snapshot_bytes(&next));
        if next.0.is_none() && next.1.as_slice().is_empty() {
            self.command_snapshots.remove(id);
        } else {
            self.command_snapshots.insert(id.to_owned(), next);
        }
    }
}

fn command_snapshot_bytes((output, locations): &CommandSnapshot) -> usize {
    output.as_ref().map_or(0, |value| value.as_str().len())
        + locations
            .as_slice()
            .iter()
            .map(|location| match location {
                SafeResourceLocation::Resource { resource } => {
                    resource.root_id.as_str().len() + resource.relative_path.as_str().len() + 64
                }
                SafeResourceLocation::Unavailable { label, .. } => label.as_str().len() + 64,
            })
            .sum::<usize>()
}

#[derive(Debug, Clone)]
pub(super) struct CommandPatch {
    output: Option<Option<SafeText<65536>>>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
}

impl CommandPatch {
    pub(super) fn locations_only(
        locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
    ) -> Self {
        Self {
            output: None,
            locations,
        }
    }

    pub(super) fn retains_output(&self, (output, _): &CommandSnapshot) -> bool {
        match self.output.as_ref() {
            None => true,
            Some(next) => {
                next.as_ref().map(SafeText::as_str) == output.as_ref().map(SafeText::as_str)
            }
        }
    }

    pub(super) fn apply(self, (output, locations): CommandSnapshot) -> CommandSnapshot {
        (
            self.output.unwrap_or(output),
            self.locations.unwrap_or(locations),
        )
    }
}

pub(super) fn path_locations(
    category: ToolCategory,
    update: &Map<String, Value>,
    resources: &impl ResourceMapper,
) -> Result<Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>, TranslationError> {
    if !matches!(
        category,
        ToolCategory::Read
            | ToolCategory::Edit
            | ToolCategory::Delete
            | ToolCategory::Move
            | ToolCategory::Search
            | ToolCategory::Execute
    ) {
        return Ok(None);
    }
    match update.get("locations") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(Default::default())),
        Some(Value::Array(locations)) => {
            if locations.len() > COMMAND_RESOURCE_LIMIT {
                return Err(TranslationError::ToolLimit);
            }
            let mut mapped = Vec::with_capacity(locations.len());
            for location in locations {
                let path = location
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or(TranslationError::MalformedFrame)?;
                mapped.push(match resources.map_absolute(path) {
                    Ok(resource) => SafeResourceLocation::Resource { resource },
                    Err(reason) => SafeResourceLocation::unavailable(reason),
                });
            }
            Ok(Some(BoundedVec::<
                SafeResourceLocation,
                { COMMAND_RESOURCE_LIMIT },
            >::new("toolLocations", mapped)?))
        }
        Some(_) => Err(TranslationError::MalformedFrame),
    }
}

pub(super) fn command_update_payload(
    update: &Map<String, Value>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
) -> Result<CommandPatch, TranslationError> {
    if let Some(output) = update.get("rawOutput").and_then(Value::as_str) {
        return Ok(CommandPatch {
            output: Some(Some(retained_output(output)?)),
            locations,
        });
    }
    output_patch(update, locations, command_block_text, true)
}

pub(super) fn content_update_payload(
    update: &Map<String, Value>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
) -> Result<CommandPatch, TranslationError> {
    output_patch(update, locations, content_block_text, false)
}

fn command_block_text(entry: &Value) -> Option<&str> {
    (entry.get("type").and_then(Value::as_str) == Some("text"))
        .then(|| entry.get("text").and_then(Value::as_str))
        .flatten()
        .or_else(|| {
            content_block_text(entry).map(|text| {
                text.strip_prefix("```console\n")
                    .and_then(|body| body.strip_suffix("\n```"))
                    .unwrap_or(text)
            })
        })
}

fn content_block_text(entry: &Value) -> Option<&str> {
    let content = (entry.get("type").and_then(Value::as_str)
        == Some(ToolCallContentKind::Content.as_wire()))
    .then(|| entry.get("content"))
    .flatten()?;
    (content.get("type").and_then(Value::as_str) == Some("text"))
        .then(|| content.get("text").and_then(Value::as_str))
        .flatten()
}

fn output_patch(
    update: &Map<String, Value>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
    text_of: fn(&Value) -> Option<&str>,
    preserve_empty: bool,
) -> Result<CommandPatch, TranslationError> {
    let output = match update.get("content") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::Array(content)) => {
            let mut blocks = content.iter().filter_map(text_of).peekable();
            let emitted = blocks.peek().is_some();
            let joined =
                join_command_output_blocks(blocks).ok_or(TranslationError::MalformedFrame)?;
            Some(
                (emitted && (preserve_empty || !joined.is_empty()))
                    .then(|| retained_output(&joined))
                    .transpose()?,
            )
        }
        Some(_) => return Err(TranslationError::MalformedFrame),
    };
    Ok(CommandPatch { output, locations })
}

fn retained_output(output: &str) -> Result<SafeText<65536>, TranslationError> {
    let sanitized = sanitized_command_output(output)?;
    let text = retain_command_output(&sanitized)
        .and_then(|retained| retained.display_text())
        .ok_or(TranslationError::MalformedFrame)?;
    normalized_text::<65536>("toolOutput", &text).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use super::super::AdapterEvent;
    use crate::acp::intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason};

    struct NoResources;

    impl ResourceMapper for NoResources {
        fn map_absolute(
            &self,
            _absolute_path: &str,
        ) -> Result<SafeResourceRef, UnavailableResourceReason> {
            Err(UnavailableResourceReason::Foreign)
        }
    }

    fn saturated_translator(session: &str) -> StandardAcpTranslator {
        let profile = super::super::super::profile::profile_for("claude-code").expect("profile");
        let mut translator = StandardAcpTranslator::new(profile, session).expect("translator");
        let exact = SafeText::new("toolOutput", "x".repeat(65_536)).expect("exact output");
        for index in 0..128 {
            translator.command_snapshots.insert(
                format!("command-{index}"),
                (Some(exact.clone()), Default::default()),
            );
            translator.command_snapshot_bytes += exact.as_str().len();
        }
        translator
    }

    fn frame(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("frame object")
    }

    fn tool_output(events: &[AdapterEvent]) -> Option<SafeText<65536>> {
        events
            .iter()
            .find_map(|event| match event {
                AdapterEvent::Item { payload, .. } => Some(payload),
                _ => None,
            })
            .and_then(|payload| match payload {
                NormalizedPayload::ToolCall { output_summary, .. } => output_summary.clone(),
                _ => None,
            })
    }

    fn tool_locations(events: &[AdapterEvent]) -> usize {
        events
            .iter()
            .find_map(|event| match event {
                AdapterEvent::Item {
                    payload: NormalizedPayload::ToolCall { locations, .. },
                    ..
                } => Some(locations.as_slice().len()),
                _ => None,
            })
            .expect("tool call payload")
    }

    #[test]
    fn over_budget_generic_tool_output_degrades_instead_of_failing_the_turn() {
        let mut translator = saturated_translator("claude-read-cap");
        let saturated = translator.command_snapshot_bytes;
        let started = translator
            .tool_call(
                &frame(json!({
                    "toolCallId": "read-1",
                    "kind": "read",
                    "status": "pending",
                    "title": "Read file",
                    "locations": [{"path": "/fixture/root/src/lib.rs", "line": 1}]
                })),
                &NoResources,
            )
            .expect("read tool call with locations admitted at the cap");
        assert_eq!(tool_locations(&started), 1);
        let events = translator
            .tool_update(
                &frame(json!({
                    "toolCallId": "read-1",
                    "status": "completed",
                    "locations": [{"path": "/fixture/root/src/lib.rs", "line": 1}],
                    "content": [{
                        "type": "content",
                        "content": {"type": "text", "text": "read-only harness probe"}
                    }]
                })),
                &NoResources,
            )
            .expect("over-budget read output degrades instead of failing the turn");
        assert_eq!(tool_output(&events), None);
        assert_eq!(tool_locations(&events), 1);
        assert!(!translator.command_snapshots.contains_key("read-1"));
        assert_eq!(translator.command_snapshot_bytes, saturated);

        let sparse = translator
            .tool_update(
                &frame(json!({
                    "toolCallId": "read-1",
                    "status": "completed",
                    "title": "Read file (1 line)"
                })),
                &NoResources,
            )
            .expect("sparse completion survives the saturated budget");
        assert!(matches!(sparse.as_slice(), [AdapterEvent::IgnoredLate]));
    }

    #[test]
    fn fetch_output_summary_stays_href_only_and_never_promotes_content_text() {
        let profile = super::super::super::profile::profile_for("claude-code").expect("profile");
        let mut translator =
            StandardAcpTranslator::new(profile, "claude-fetch-authority").expect("translator");
        translator
            .tool_call(
                &frame(json!({
                    "toolCallId": "fetch-1",
                    "kind": "fetch",
                    "status": "pending",
                    "title": "Fetch page"
                })),
                &NoResources,
            )
            .expect("fetch tool call admitted");
        let spoofed = translator
            .tool_update(
                &frame(json!({
                    "toolCallId": "fetch-1",
                    "status": "in_progress",
                    "content": [{
                        "type": "content",
                        "content": {"type": "text", "text": "http://127.0.0.1:5173/app"}
                    }]
                })),
                &NoResources,
            )
            .expect("fetch content text translated");
        assert_eq!(tool_output(&spoofed), None);
        let admitted = translator
            .tool_update(
                &frame(json!({
                    "toolCallId": "fetch-1",
                    "status": "completed",
                    "url": "http://127.0.0.1:5173/app"
                })),
                &NoResources,
            )
            .expect("structured fetch href translated");
        assert_eq!(
            tool_output(&admitted).as_ref().map(SafeText::as_str),
            Some("http://127.0.0.1:5173/app")
        );
    }

    #[test]
    fn over_budget_execute_output_still_fails_the_turn() {
        let mut translator = saturated_translator("claude-execute-cap");
        translator
            .tool_call(
                &frame(json!({
                    "toolCallId": "run-1",
                    "kind": "execute",
                    "status": "pending",
                    "title": "Run command"
                })),
                &NoResources,
            )
            .expect("execute tool call admitted at the cap");
        assert!(matches!(
            translator.tool_update(
                &frame(json!({
                    "toolCallId": "run-1",
                    "status": "completed",
                    "content": [{"type": "text", "text": "command harness probe"}]
                })),
                &NoResources,
            ),
            Err(TranslationError::ItemLimit)
        ));
    }

    #[test]
    fn command_snapshot_aggregate_cap_accepts_exact_and_rejects_plus_one() {
        let profile = super::super::super::profile::profile_for("claude-code").expect("profile");
        let mut translator =
            StandardAcpTranslator::new(profile, "claude-command-cap").expect("translator");
        let exact = SafeText::new("toolOutput", "x".repeat(65_536)).expect("exact output");
        for index in 0..128 {
            translator.command_snapshots.insert(
                format!("command-{index}"),
                (Some(exact.clone()), Default::default()),
            );
            translator.command_snapshot_bytes += exact.as_str().len();
        }
        assert_eq!(
            translator.command_snapshot_bytes,
            LIMITS.wire.item_bytes_per_turn as usize
        );
        assert!(matches!(
            translator.validate_command_snapshot(
                "plus-one",
                &(
                    Some(SafeText::new("toolOutput", "x").expect("byte")),
                    Default::default()
                )
            ),
            Err(TranslationError::ItemLimit)
        ));
    }
}
