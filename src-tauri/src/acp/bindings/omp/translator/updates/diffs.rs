use serde_json::{Map, Value};

use crate::acp::{
    contract::{ItemLifecycle, RequiredText, SafeText},
    intent::{FileChangeFacts, FileChangeOperation, NormalizedPayload},
};

use super::super::{ResourceMapper, TranslationError};
use super::MAX_EVENTS_PER_FRAME;
use super::kinds::line_count;
use crate::acp::bindings::omp::protocol::{AdapterEvent, ItemUpdateMode};

pub(super) fn push_diff_events(
    id: &str,
    update: &Map<String, Value>,
    resources: &impl ResourceMapper,
    events: &mut Vec<AdapterEvent>,
) -> Result<(), TranslationError> {
    let Some(content) = update.get("content").and_then(Value::as_array) else {
        return Ok(());
    };
    if content.len() >= MAX_EVENTS_PER_FRAME {
        return Err(TranslationError::Limit("tool content entries"));
    }
    for (index, entry) in content.iter().enumerate() {
        if entry.get("type").and_then(Value::as_str) != Some("diff") {
            continue;
        }
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
        let resource = match resources.map_absolute(path) {
            Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
            Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
        };
        let new_text = entry
            .get("newText")
            .and_then(Value::as_str)
            .ok_or(TranslationError::MalformedFrame)?;
        let external_key = RequiredText::new("externalItemId", format!("{id}:diff:{index}"))?;
        let summary = SafeText::new("fileSummary", "File change reported by OMP ACP")?;
        // An unavailable resource has no immutable root/path key,
        // so no pair is retained: typed unavailable, never text
        // invented from disk or Git.
        if !matches!(
            resource,
            crate::acp::intent::SafeResourceLocation::Resource { .. }
        ) {
            events.push(AdapterEvent::Item {
                external_key,
                lifecycle: ItemLifecycle::Completed,
                tool_state: None,
                payload: NormalizedPayload::file_summary_only(
                    FileChangeFacts {
                        resource,
                        summary,
                        additions: line_count(Some(new_text)),
                        deletions: line_count(Some(old_text)),
                        operation,
                    },
                    false,
                ),
                update_mode: ItemUpdateMode::Replace,
            });
            continue;
        }
        match crate::acp_protocol::file_diff::admit_pair(old_text, new_text) {
            crate::acp_protocol::file_diff::PairAdmission::Admitted {
                old_bytes,
                new_bytes,
                digest,
            } => {
                events.push(AdapterEvent::Item {
                    external_key: external_key.clone(),
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::file_admitted(
                        FileChangeFacts {
                            resource,
                            summary,
                            additions: line_count(Some(new_text)),
                            deletions: line_count(Some(old_text)),
                            operation,
                        },
                        0,
                        (old_bytes as u64, new_bytes as u64),
                        digest,
                    ),
                    update_mode: ItemUpdateMode::Replace,
                });
                events.push(AdapterEvent::FilePair {
                    external_key,
                    old_text: old_text.to_owned(),
                    new_text: new_text.to_owned(),
                    update_mode: ItemUpdateMode::Replace,
                });
            }
            crate::acp_protocol::file_diff::PairAdmission::SummaryOnly => {
                events.push(AdapterEvent::Item {
                    external_key,
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: None,
                    payload: NormalizedPayload::file_summary_only(
                        FileChangeFacts {
                            resource,
                            summary,
                            additions: line_count(Some(new_text)),
                            deletions: line_count(Some(old_text)),
                            operation,
                        },
                        true,
                    ),
                    update_mode: ItemUpdateMode::Replace,
                });
            }
        }
    }
    Ok(())
}
