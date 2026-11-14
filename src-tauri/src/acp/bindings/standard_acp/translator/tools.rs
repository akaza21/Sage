use serde_json::{Map, Value};

use crate::acp::runtime::host_browse::browse_tool_summary;
use sage_acp_protocol::{
    contract::{BoundedVec, RequiredText, SafeText},
    intent::{
        ExternalId, FileChangeOperation, NormalizedPayload, SafeResourceLocation, ToolCategory,
    },
};
use sage_core::limits::COMMAND_RESOURCE_LIMIT;
use sage_core::limits::LIMITS;

use super::super::sanitize::normalized_label_text;
use super::decode::{
    category_label, lifecycle_for_tool, parse_tool_state, string, structural_subagent,
    transition_tool,
};
use super::diff_records::{DiffObservation, FrameState};
use super::tool_input::{command_input_patch, web_search_query};
use super::tool_snapshots::{
    CommandPatch, command_update_payload, content_update_payload, path_locations,
};
use super::{
    AdapterEvent, MAX_TRACKED_TOOLS, ResourceMapper, StandardAcpTranslator, TranslationError,
};

pub(super) const POST_TERMINAL_TOOL_MUTATION: &str =
    "ACP harness mutated a settled tool call after it reached a terminal state";

impl StandardAcpTranslator {
    pub(super) fn tool_call(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id: ExternalId = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
        let category = update
            .get("kind")
            .and_then(Value::as_str)
            .map_or(ToolCategory::Other, ToolCategory::from_wire);
        let category = if category == ToolCategory::Other && structural_subagent(update) {
            ToolCategory::Subagent
        } else {
            category
        };
        let category = if category == ToolCategory::Other
            && update
                .get("title")
                .and_then(Value::as_str)
                .is_some_and(|title| title.eq_ignore_ascii_case("PowerShell"))
        {
            ToolCategory::Execute
        } else if category == ToolCategory::Other && verified_write_file(update, resources) {
            ToolCategory::Edit
        } else if category == ToolCategory::Fetch && web_search_query(update).is_some() {
            ToolCategory::WebSearch
        } else {
            category
        };
        let state = parse_tool_state(
            update.get("status"),
            sage_acp_protocol::lifecycle::ToolState::Pending,
        )?;
        if let Some(current) = self.tools.get(id.as_str()).copied() {
            if current == state {
                return Ok(vec![AdapterEvent::IgnoredDuplicate]);
            }
            return Err(TranslationError::DuplicateTool);
        }
        if self.tools.len() >= MAX_TRACKED_TOOLS
            || self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ToolLimit);
        }
        let command = category == ToolCategory::Execute;
        let protocol_locations = path_locations(category, update, resources)?;
        let snapshot =
            output_payload(category, update, protocol_locations)?.apply((None, Default::default()));
        let locations = snapshot.1.clone();
        let retained = self.admit_snapshot(id.as_str(), snapshot, command)?;
        self.tools.insert(id.as_str().to_owned(), state);
        self.tool_categories
            .insert(id.as_str().to_owned(), category);
        self.store_command_snapshot(id.as_str(), retained);
        let (output_summary, _) = self.command_snapshot(id.as_str());
        let input_summary = command_input_patch(category, update)
            .flatten()
            .or_else(|| browse_tool_summary("toolInput", update));
        if let Some(summary) = input_summary.as_ref() {
            self.tool_inputs
                .insert(id.as_str().to_owned(), summary.clone());
        }
        let label = self.resolve_tool_label(id.as_str(), update, category)?;
        let frame = FrameState {
            settling: state.is_terminal(),
            late: false,
        };
        let mut outcome = self.content_events(&id, update, resources, frame)?;
        // Only a stated label persists; a fallback is derived again each
        // frame so a command line reported later can replace it.
        let label =
            match file_action_label(update, category, &locations, &outcome.observed, resources)? {
                Some(file) => {
                    self.tool_labels
                        .insert(id.as_str().to_owned(), file.clone());
                    file
                }
                None => label,
            };
        self.record_diffs(id.as_str(), std::mem::take(&mut outcome.observed));
        self.record_command_outcome(
            id.as_str(),
            category,
            update,
            outcome.terminal_output.as_deref(),
        )?;
        let execution = self.tool_execution(id.as_str(), category)?;
        let mut events = Vec::with_capacity(outcome.events.len().saturating_add(1));
        events.push(AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle_for_tool(state),
            tool_state: Some(state),
            payload: NormalizedPayload::ToolCall {
                execution,
                category,
                label,
                input_summary,
                output_summary,
                locations,
                command_activity: Default::default(),
            },
        });
        events.append(&mut outcome.events);
        self.settle_diffs(id.as_str(), state, Some(resources), &mut events)?;
        Ok(events)
    }

    pub(super) fn tool_update(
        &mut self,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
    ) -> Result<Vec<AdapterEvent>, TranslationError> {
        let id: ExternalId = RequiredText::new("toolCallId", string(update, "toolCallId")?)?;
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
        let category = if category == ToolCategory::Other && verified_write_file(update, resources)
        {
            ToolCategory::Edit
        } else {
            category
        };
        let command = category == ToolCategory::Execute;
        let protocol_locations = path_locations(category, update, resources)?;
        let patch = output_payload(category, update, protocol_locations.clone())?;
        let input_summary = command_input_patch(category, update);
        let input_changed = input_summary
            .as_ref()
            .is_some_and(|input| self.tool_inputs.get(id.as_str()) != input.as_ref());
        let exit_reported = self.reports_new_exit_code(id.as_str(), category, update);
        let frame = FrameState {
            settling: next.is_terminal(),
            late: current.is_terminal(),
        };
        let mut outcome = self.content_events(&id, update, resources, frame)?;
        let file_label = file_action_label(
            update,
            category,
            &protocol_locations.clone().unwrap_or_default(),
            &outcome.observed,
            resources,
        )?;
        let refreshed = self.record_diffs(id.as_str(), std::mem::take(&mut outcome.observed));
        if current.is_terminal() {
            let reported = update
                .get("status")
                .is_some()
                .then_some(next)
                .filter(|reported| *reported != current);
            if let Some(reported) = reported {
                if !self.host_settled_tool(id.as_str()) {
                    return Err(TranslationError::TerminalConflict);
                }
                if reported.is_terminal() {
                    match self.settled_reports.get(id.as_str()) {
                        Some(previous) if *previous != reported => {
                            return Err(TranslationError::TerminalConflict);
                        }
                        Some(_) => {}
                        None => {
                            self.settled_reports
                                .insert(id.as_str().to_owned(), reported);
                        }
                    }
                }
            }
            let mutating = !patch.retains_output(&self.command_snapshot(id.as_str()))
                || input_changed
                || exit_reported
                || protocol_locations.is_some()
                || outcome.terminal_output.is_some()
                || update.get("title").is_some();
            if mutating {
                tracing::warn!(tool_state = ?current, "{POST_TERMINAL_TOOL_MUTATION}");
            }
            let mut settled = std::mem::take(&mut outcome.events);
            self.settle_diffs(id.as_str(), current, Some(resources), &mut settled)?;
            if !settled.is_empty() {
                return Ok(settled);
            }
            if mutating || refreshed || reported.is_some() {
                return Ok(vec![AdapterEvent::IgnoredLate]);
            }
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        if next == current
            && !input_changed
            && !exit_reported
            && patch.retains_output(&self.command_snapshot(id.as_str()))
            && protocol_locations.is_none()
            && outcome.events.is_empty()
            && !refreshed
            && outcome.terminal_output.is_none()
            && update.get("title").is_none()
        {
            return Ok(vec![AdapterEvent::IgnoredDuplicate]);
        }
        if next != current {
            transition_tool(current, next)?;
        }
        let merged = patch.apply(self.command_snapshot(id.as_str()));
        let locations = merged.1.clone();
        let (retained_output, retained_locations) =
            self.admit_snapshot(id.as_str(), merged, command)?;
        if let Some(input) = input_summary {
            self.set_command_input(id.as_str(), input);
        }
        self.record_command_outcome(
            id.as_str(),
            category,
            update,
            outcome.terminal_output.as_deref(),
        )?;
        let execution = self.tool_execution(id.as_str(), category)?;
        let label = match file_label {
            Some(label) => {
                self.tool_labels
                    .insert(id.as_str().to_owned(), label.clone());
                label
            }
            None => self.resolve_tool_label(id.as_str(), update, category)?,
        };
        let output_summary = match category {
            ToolCategory::Execute => retained_output.clone(),
            ToolCategory::Fetch => browse_tool_summary("toolOutput", update),
            _ => retained_output
                .clone()
                .or_else(|| browse_tool_summary("toolOutput", update)),
        };
        let mut events = Vec::with_capacity(outcome.events.len().saturating_add(1));
        events.push(AdapterEvent::Item {
            external_key: id.clone(),
            lifecycle: lifecycle_for_tool(next),
            tool_state: Some(next),
            payload: NormalizedPayload::ToolCall {
                execution,
                category,
                label,
                input_summary: self.tool_inputs.get(id.as_str()).cloned(),
                output_summary,
                locations,
                command_activity: Default::default(),
            },
        });
        events.append(&mut outcome.events);
        if next != current {
            self.tools.insert(id.as_str().to_owned(), next);
        }
        self.tool_categories
            .insert(id.as_str().to_owned(), category);
        self.store_command_snapshot(id.as_str(), (retained_output, retained_locations));
        self.settle_diffs(id.as_str(), next, Some(resources), &mut events)?;
        Ok(events)
    }
}

fn verified_write_file(update: &Map<String, Value>, resources: &impl ResourceMapper) -> bool {
    if update.get("kind").and_then(Value::as_str) != Some("write") {
        return false;
    }
    let mapped_location = update
        .get("locations")
        .and_then(Value::as_array)
        .filter(|locations| locations.len() == 1)
        .and_then(|locations| locations[0].get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| resources.map_absolute(path).is_ok());
    let mapped_diff = update
        .get("content")
        .and_then(Value::as_array)
        .filter(|content| content.len() == 1)
        .and_then(|content| content[0].as_object())
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("diff"))
        .and_then(|content| content.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| resources.map_absolute(path).is_ok());
    mapped_location || mapped_diff
}

fn file_action_label(
    update: &serde_json::Map<String, serde_json::Value>,
    category: ToolCategory,
    locations: &BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    observed: &[(String, DiffObservation)],
    resources: &impl ResourceMapper,
) -> Result<Option<SafeText<512>>, TranslationError> {
    if category == ToolCategory::Execute {
        return Ok(None);
    }
    let (operation, location) = if observed.len() == 1 {
        let facts = &observed[0].1.facts;
        let location = match resources.map_absolute(&facts.native_path) {
            Ok(resource) => SafeResourceLocation::Resource { resource },
            Err(reason) => SafeResourceLocation::unavailable(reason),
        };
        (facts.operation, location)
    } else if observed.is_empty() && locations.as_slice().len() == 1 {
        let operation = match update.get("kind").and_then(serde_json::Value::as_str) {
            Some("write") => FileChangeOperation::Created,
            Some("edit") if category == ToolCategory::Edit => FileChangeOperation::Updated,
            _ => return Ok(None),
        };
        (operation, locations.as_slice()[0].clone())
    } else {
        return Ok(None);
    };
    Ok(Some(SafeText::new(
        "toolLabel",
        operation.describe(&location),
    )?))
}

impl StandardAcpTranslator {
    fn resolve_tool_label(
        &mut self,
        id: &str,
        update: &Map<String, Value>,
        category: ToolCategory,
    ) -> Result<SafeText<512>, TranslationError> {
        if let Some(title) = update
            .get("title")
            .and_then(Value::as_str)
            .filter(|title| !title.trim().is_empty())
        {
            let label = normalized_label_text::<512>("toolLabel", title)?;
            self.tool_labels.insert(id.to_owned(), label.clone());
            return Ok(label);
        }
        self.stated_or_fallback_label(id, category)
    }

    pub(super) fn stated_or_fallback_label(
        &self,
        id: &str,
        category: ToolCategory,
    ) -> Result<SafeText<512>, TranslationError> {
        if let Some(label) = self.tool_labels.get(id) {
            return Ok(label.clone());
        }
        if category == ToolCategory::Execute
            && let Some(command) = self.tool_inputs.get(id)
            && let Ok(label) = normalized_label_text::<512>("toolLabel", command.as_str())
        {
            return Ok(label);
        }
        SafeText::new("toolLabel", category_label(self.profile, category)).map_err(Into::into)
    }
}

fn output_payload(
    category: ToolCategory,
    update: &Map<String, Value>,
    locations: Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>,
) -> Result<CommandPatch, TranslationError> {
    match category {
        ToolCategory::Execute => command_update_payload(update, locations),
        ToolCategory::Fetch => Ok(CommandPatch::locations_only(locations)),
        _ => content_update_payload(update, locations),
    }
}
