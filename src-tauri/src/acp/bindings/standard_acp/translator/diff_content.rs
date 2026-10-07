use serde_json::{Map, Value};

use crate::acp::{
    contract::{RequiredText, SafeText},
    intent::{
        ExternalId, FileChangeFacts, FileChangeOperation, NormalizedPayload, SafeResourceLocation,
        UnavailableResourceReason,
    },
    lifecycle::ItemLifecycle,
};

use super::super::sanitize::sanitized_command_output;
use super::super::surface::ToolCallContentKind;
use super::decode::line_count;
use super::diff_records::{DiffFacts, DiffObservation, DiffRecord, FrameState};
use super::tools::FILE_CHANGE_SUMMARY;
use super::{
    AdapterEvent, MAX_EVENTS_PER_FRAME, ResourceMapper, StandardAcpTranslator, TranslationError,
};

#[derive(Default)]
pub(super) struct ContentOutcome {
    pub(super) events: Vec<AdapterEvent>,
    pub(super) terminal_output: Option<String>,
    pub(super) observed: Vec<(String, DiffObservation)>,
}

impl StandardAcpTranslator {
    pub(super) fn content_events(
        &self,
        id: &ExternalId,
        update: &Map<String, Value>,
        resources: &impl ResourceMapper,
        frame: FrameState,
    ) -> Result<ContentOutcome, TranslationError> {
        let Some(content) = update.get("content").and_then(Value::as_array) else {
            return Ok(ContentOutcome::default());
        };
        if content.len() >= MAX_EVENTS_PER_FRAME {
            return Err(TranslationError::EventLimit);
        }
        let mut sink = ContentOutcome::default();
        for (index, entry) in content.iter().enumerate() {
            match entry
                .get("type")
                .and_then(Value::as_str)
                .and_then(ToolCallContentKind::parse)
            {
                Some(ToolCallContentKind::Diff) => {
                    self.diff_events(id, index, entry, resources, frame, &mut sink)?;
                }
                Some(ToolCallContentKind::Terminal) => {
                    if let Some(output) = entry.get("output").and_then(Value::as_str) {
                        sink.terminal_output = Some(sanitized_command_output(output)?);
                    }
                }
                _ => {}
            }
        }
        Ok(sink)
    }

    fn diff_events(
        &self,
        id: &ExternalId,
        index: usize,
        entry: &Value,
        resources: &impl ResourceMapper,
        frame: FrameState,
        sink: &mut ContentOutcome,
    ) -> Result<(), TranslationError> {
        let path = entry
            .get("path")
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let (old_text, operation) = match entry.get("oldText") {
            None | Some(Value::Null) => ("", FileChangeOperation::Created),
            Some(value) => (
                value.as_str().ok_or(TranslationError::MalformedFrame)?,
                FileChangeOperation::Updated,
            ),
        };
        let new_text = entry
            .get("newText")
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let external_key: ExternalId =
            RequiredText::new("externalItemId", format!("{}:diff:{index}", id.as_str()))?;
        let record = self.diff_record(external_key.as_str());
        if record.is_some_and(DiffRecord::settled) {
            return Ok(());
        }
        let poisoned = record.is_some_and(|record| record.bound_path() != path);
        let mapped = if poisoned {
            Err(record.map_or(UnavailableResourceReason::Unsupported, DiffRecord::reason))
        } else {
            resources.map_absolute(path)
        };
        let facts = DiffFacts {
            native_path: path.to_owned(),
            additions: Some(line_count(new_text)),
            deletions: Some(line_count(old_text)),
            operation,
            unavailable: mapped.as_ref().err().copied(),
        };
        let frozen = record.and_then(DiffRecord::frozen);
        let frozen_reason = record.and_then(DiffRecord::frozen_reason);
        let streamed = !frame.settling
            && !(!old_text.is_empty() && new_text.is_empty())
            && !(poisoned && frozen.is_some())
            && record.and_then(DiffRecord::streamed) != Some(&facts);
        let resource = match frozen {
            Some(resource) => SafeResourceLocation::Resource {
                resource: resource.clone(),
            },
            None => match &mapped {
                Ok(resource) => SafeResourceLocation::Resource {
                    resource: resource.clone(),
                },
                Err(reason) => SafeResourceLocation::unavailable(frozen_reason.unwrap_or(*reason)),
            },
        };
        let emitted = match &resource {
            SafeResourceLocation::Resource { resource } => Some(resource.clone()),
            SafeResourceLocation::Unavailable { .. } => None,
        };
        let emitted_reason = match &resource {
            SafeResourceLocation::Resource { .. } => None,
            SafeResourceLocation::Unavailable { reason, .. } => Some(*reason),
        };
        if streamed {
            sink.events.push(AdapterEvent::Item {
                external_key: external_key.clone(),
                lifecycle: ItemLifecycle::Streaming,
                tool_state: None,
                payload: NormalizedPayload::file_summary_only(
                    FileChangeFacts {
                        resource,
                        summary: SafeText::new("fileSummary", FILE_CHANGE_SUMMARY)?,
                        additions: facts.additions,
                        deletions: facts.deletions,
                        operation,
                    },
                    false,
                ),
            });
        }
        sink.observed.push((
            external_key.as_str().to_owned(),
            DiffObservation {
                facts,
                old_text: old_text.to_owned(),
                new_text: new_text.to_owned(),
                late: frame.late,
                streamed,
                emitted: streamed.then_some(emitted).flatten(),
                emitted_reason: streamed.then_some(emitted_reason).flatten(),
            },
        ));
        Ok(())
    }
}
