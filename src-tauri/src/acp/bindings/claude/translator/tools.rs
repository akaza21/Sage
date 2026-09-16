use serde_json::{Map, Value};

use crate::acp::{
    command_output::{join_command_output_blocks, retain_command_output},
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, NormalizedPayload, SafeResourceLocation},
    lifecycle::ItemLifecycle,
    limits::LIMITS,
    runtime::host_browse::browse_tool_summary,
};

use super::{
    AdapterEvent, ClaudeTranslator, MAX_EVENTS_PER_FRAME, MAX_TRACKED_TOOLS, ResourceMapper,
    TranslationError, category_label, lifecycle_for_tool, line_count, parse_tool_state, string,
    tool_category, transition_tool,
};

impl ClaudeTranslator {
    pub(super) fn tool_call(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<AdapterEvent, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let category = tool_category(update.get("kind").and_then(Value::as_str));
        let state = parse_tool_state(
            update.get("status"),
            crate::acp::lifecycle::ToolState::Pending,
        )?;
        if let Some(current) = self.tools.get(id.as_str()).copied() {
            if current == state {
                return Ok(AdapterEvent::IgnoredDuplicate);
            }
            return Err(TranslationError::DuplicateTool);
        }
        if self.tools.len() >= MAX_TRACKED_TOOLS
            || self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ToolLimit);
        }
        let command = category == crate::acp::intent::ToolCategory::Command;
        let protocol_locations = path_locations(category, update, resources)?;
        let command_snapshot = command
            .then(|| command_update_payload(update, protocol_locations.clone()))
            .transpose()?
            .map(|patch| patch.apply((None, Default::default())));
        if let Some(snapshot) = command_snapshot.as_ref() {
            self.validate_command_snapshot(id.as_str(), snapshot)?;
        }
        let locations = protocol_locations.unwrap_or_else(|| {
            command_snapshot
                .as_ref()
                .map(|(_, locations)| locations.clone())
                .unwrap_or_default()
        });
        self.tools.insert(id.as_str().to_owned(), state);
        self.tool_categories
            .insert(id.as_str().to_owned(), category);
        if let Some(snapshot) = command_snapshot {
            self.store_command_snapshot(id.as_str(), snapshot);
        }
        let (output_summary, _) = self.command_snapshot(id.as_str());
        Ok(AdapterEvent::Item {
            external_key: id,
            lifecycle: lifecycle_for_tool(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                input_summary: browse_tool_summary("toolInput", update),
                output_summary: command.then_some(output_summary).flatten(),
                locations,
            },
        })
    }

    pub(super) fn tool_update(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let current = self
            .tools
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        let next = parse_tool_state(update.get("status"), current)?;
        let category = self
            .tool_categories
            .get(id.as_str())
            .copied()
            .ok_or(TranslationError::OrphanToolUpdate)?;
        let command = category == crate::acp::intent::ToolCategory::Command;
        let protocol_locations = path_locations(category, update, resources)?;
        let command_patch = command
            .then(|| command_update_payload(update, protocol_locations.clone()))
            .transpose()?;
        if next == current
            && command_patch.as_ref().is_none_or(CommandPatch::is_omitted)
            && protocol_locations.is_none()
        {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        if next != current {
            transition_tool(current, next)?;
        }
        let command_snapshot = if let Some(patch) = command_patch {
            let snapshot = self.command_snapshot(id.as_str());
            let merged = patch.apply(snapshot);
            self.validate_command_snapshot(id.as_str(), &merged)?;
            Some(merged)
        } else {
            None
        };
        let (output_summary, snapshot_locations) = command_snapshot
            .as_ref()
            .cloned()
            .unwrap_or_else(|| self.command_snapshot(id.as_str()));
        let output_summary = if command {
            output_summary
        } else {
            browse_tool_summary("toolOutput", update)
        };
        let locations = protocol_locations.unwrap_or(snapshot_locations);
        let mut events = vec![AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle_for_tool(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolUpdateResult {
                category,
                label: SafeText::new("toolLabel", category_label(category))?,
                output_summary,
                locations,
            },
        }];
        if let Some(content) = update.get("content").and_then(Value::as_array) {
            if content.len() >= MAX_EVENTS_PER_FRAME {
                return Err(TranslationError::EventLimit);
            }
            for (index, entry) in content.iter().enumerate() {
                if entry.get("type").and_then(Value::as_str) == Some("diff") {
                    let path = entry
                        .get("path")
                        .and_then(Value::as_str)
                        .ok_or(TranslationError::MalformedFrame)?;
                    let resource = match resources.map_absolute(path) {
                        Ok(resource) => SafeResourceLocation::Resource { resource },
                        Err(reason) => SafeResourceLocation::unavailable(reason),
                    };
                    // Missing, null, or non-string sides are malformed wire
                    // input, not an empty edit. Over-cap sides admit
                    // summary-only metadata without retaining a partial side.
                    let old_text = entry
                        .get("oldText")
                        .and_then(Value::as_str)
                        .ok_or(TranslationError::MalformedFrame)?;
                    let new_text = entry
                        .get("newText")
                        .and_then(Value::as_str)
                        .ok_or(TranslationError::MalformedFrame)?;
                    let external_key = RequiredText::new(
                        "externalItemId",
                        format!("{}:diff:{index}", id.as_str()),
                    )?;
                    let summary =
                        SafeText::new("fileSummary", "File change reported by Claude ACP")?;
                    // An unavailable resource has no immutable root/path key,
                    // so no pair is retained: typed unavailable, never text
                    // invented from disk or Git.
                    if !matches!(resource, SafeResourceLocation::Resource { .. }) {
                        events.push(AdapterEvent::Item {
                            external_key,
                            lifecycle: ItemLifecycle::Completed,
                            tool_state: None,
                            payload: NormalizedPayload::file_summary_only(
                                resource,
                                summary,
                                Some(line_count(new_text)),
                                Some(line_count(old_text)),
                                false,
                            ),
                        });
                        continue;
                    }
                    match crate::acp::file_diff::admit_pair(old_text, new_text) {
                        crate::acp::file_diff::PairAdmission::Admitted {
                            old_bytes,
                            new_bytes,
                            digest,
                        } => {
                            events.push(AdapterEvent::Item {
                                external_key: external_key.clone(),
                                lifecycle: ItemLifecycle::Completed,
                                tool_state: None,
                                payload: NormalizedPayload::file_admitted(
                                    resource,
                                    summary,
                                    Some(line_count(new_text)),
                                    Some(line_count(old_text)),
                                    0,
                                    (old_bytes as u64, new_bytes as u64),
                                    digest,
                                ),
                            });
                            events.push(AdapterEvent::FilePair {
                                external_key,
                                old_text: old_text.to_owned(),
                                new_text: new_text.to_owned(),
                            });
                        }
                        crate::acp::file_diff::PairAdmission::SummaryOnly => {
                            events.push(AdapterEvent::Item {
                                external_key,
                                lifecycle: ItemLifecycle::Completed,
                                tool_state: None,
                                payload: NormalizedPayload::file_summary_only(
                                    resource,
                                    summary,
                                    Some(line_count(new_text)),
                                    Some(line_count(old_text)),
                                    true,
                                ),
                            });
                        }
                    }
                }
            }
        }
        if next != current {
            self.tools.insert(id.as_str().to_owned(), next);
        }
        if let Some(snapshot) = command_snapshot {
            self.store_command_snapshot(id.as_str(), snapshot);
        }
        Ok(events)
    }
}

impl ClaudeTranslator {
    fn command_snapshot(
        &self,
        id: &str,
    ) -> (
        Option<SafeText<65536>>,
        BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    ) {
        self.command_snapshots
            .get(id)
            .cloned()
            .unwrap_or((None, Default::default()))
    }

    fn validate_command_snapshot(
        &self,
        id: &str,
        next: &(
            Option<SafeText<65536>>,
            BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        ),
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

    fn store_command_snapshot(
        &mut self,
        id: &str,
        next: (
            Option<SafeText<65536>>,
            BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        ),
    ) {
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

fn command_snapshot_bytes(
    (output, locations): &(
        Option<SafeText<65536>>,
        BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    ),
) -> usize {
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
struct CommandPatch {
    output: Option<Option<SafeText<65536>>>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
}

impl CommandPatch {
    fn is_omitted(&self) -> bool {
        self.output.is_none() && self.locations.is_none()
    }

    fn apply(
        self,
        (output, locations): (
            Option<SafeText<65536>>,
            BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
        ),
    ) -> (
        Option<SafeText<65536>>,
        BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    ) {
        (
            self.output.unwrap_or(output),
            self.locations.unwrap_or(locations),
        )
    }
}

/// Maps path-bearing protocol locations without retaining native paths.
/// Omission is distinct from an explicit empty/null replacement on updates.
fn path_locations(
    category: crate::acp::intent::ToolCategory,
    update: &Map<String, Value>,
    resources: &impl ResourceMapper,
) -> Result<Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>, TranslationError> {
    if !matches!(
        category,
        crate::acp::intent::ToolCategory::File
            | crate::acp::intent::ToolCategory::Diff
            | crate::acp::intent::ToolCategory::Command
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

fn command_update_payload(
    update: &Map<String, Value>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
) -> Result<CommandPatch, TranslationError> {
    let output = match update.get("content") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::Array(content)) => {
            let joined = join_command_output_blocks(content.iter().filter_map(|entry| {
                (entry.get("type").and_then(Value::as_str) == Some("text"))
                    .then(|| entry.get("text").and_then(Value::as_str))
                    .flatten()
            }))
            .ok_or(TranslationError::MalformedFrame)?;
            let sanitized = super::super::sanitize::sanitized_command_output(&joined)?;
            let text = retain_command_output(&sanitized)
                .and_then(|retained| retained.display_text())
                .ok_or(TranslationError::MalformedFrame)?;
            Some(
                (!text.is_empty())
                    .then(|| super::super::sanitize::normalized_text::<65536>("toolOutput", &text))
                    .transpose()?,
            )
        }
        Some(_) => return Err(TranslationError::MalformedFrame),
    };
    Ok(CommandPatch { output, locations })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_snapshot_aggregate_cap_accepts_exact_and_rejects_plus_one() {
        let mut translator = ClaudeTranslator::new("claude-command-cap").expect("translator");
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
