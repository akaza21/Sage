use serde_json::{Map, Value};

use crate::acp::{
    command_output::{join_command_output_blocks, retain_command_output},
    contract::{BoundedVec, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation, ToolCategory},
    limits::LIMITS,
};

use super::{OmpTranslator, ResourceMapper, TranslationError};
use crate::acp::bindings::omp::wire::{normalized_text, sanitized_command_output};

pub(super) type ToolSnapshot = (
    Option<SafeText<65536>>,
    BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
);

impl OmpTranslator {
    pub(super) fn tool_snapshot(&mut self, id: &str) -> Result<ToolSnapshot, TranslationError> {
        Ok(self
            .live_turn()?
            .tool_snapshots
            .get(id)
            .cloned()
            .unwrap_or((None, Default::default())))
    }

    pub(super) fn validate_tool_snapshot(
        &mut self,
        id: &str,
        next: &ToolSnapshot,
    ) -> Result<(), TranslationError> {
        let active = self.live_turn()?;
        let old = active
            .tool_snapshots
            .get(id)
            .map(tool_snapshot_bytes)
            .unwrap_or(0);
        let total = active
            .tool_snapshot_bytes
            .checked_sub(old)
            .and_then(|value| value.checked_add(tool_snapshot_bytes(next)))
            .ok_or(TranslationError::Limit("tool snapshot bytes"))?;
        (total <= LIMITS.wire.item_bytes_per_turn as usize)
            .then_some(())
            .ok_or(TranslationError::Limit("tool snapshot bytes"))
    }

    pub(super) fn store_tool_snapshot(
        &mut self,
        id: &str,
        next: ToolSnapshot,
    ) -> Result<(), TranslationError> {
        let active = self.live_turn()?;
        let old = active
            .tool_snapshots
            .get(id)
            .map(tool_snapshot_bytes)
            .unwrap_or(0);
        active.tool_snapshot_bytes = active
            .tool_snapshot_bytes
            .saturating_sub(old)
            .saturating_add(tool_snapshot_bytes(&next));
        if next.0.is_none() && next.1.as_slice().is_empty() {
            active.tool_snapshots.remove(id);
        } else {
            active.tool_snapshots.insert(id.to_owned(), next);
        }
        Ok(())
    }
}

fn tool_snapshot_bytes((output, locations): &ToolSnapshot) -> usize {
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
    pub(super) fn is_omitted(&self) -> bool {
        self.output.is_none() && self.locations.is_none()
    }

    pub(super) fn apply(self, (output, locations): ToolSnapshot) -> ToolSnapshot {
        (
            self.output.unwrap_or(output),
            self.locations.unwrap_or(locations),
        )
    }
}

/// Maps path-bearing protocol locations without retaining native paths.
/// Omission is distinct from an explicit empty/null replacement on updates.
pub(super) fn path_locations(
    category: ToolCategory,
    update: &Map<String, Value>,
    resources: &impl ResourceMapper,
) -> Result<Option<BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>>, TranslationError> {
    if !matches!(
        category,
        ToolCategory::Read | ToolCategory::Edit | ToolCategory::Execute
    ) {
        return Ok(None);
    }
    match update.get("locations") {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(Default::default())),
        Some(Value::Array(locations)) => {
            if locations.len() > COMMAND_RESOURCE_LIMIT {
                return Err(TranslationError::Limit("tool locations"));
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
            Ok(Some(BoundedVec::new("toolLocations", mapped)?))
        }
        Some(_) => Err(TranslationError::MalformedFrame),
    }
}

pub(super) fn command_update_payload(
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
            .ok_or(TranslationError::Limit("command output"))?;
            let sanitized = sanitized_command_output(&joined)?;
            let text = retain_command_output(&sanitized)
                .and_then(|retained| retained.display_text())
                .ok_or(TranslationError::Limit("command output"))?;
            Some(
                (!text.is_empty())
                    .then(|| normalized_text::<65536>("toolOutput", &text))
                    .transpose()?,
            )
        }
        Some(_) => return Err(TranslationError::MalformedFrame),
    };
    Ok(CommandPatch { output, locations })
}
