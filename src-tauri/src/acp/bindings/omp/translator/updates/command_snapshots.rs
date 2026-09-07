use serde_json::{Map, Value};

use crate::acp::{
    command_output::{join_command_output_blocks, retain_command_output},
    contract::{BoundedVec, SafeText},
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation},
    limits::LIMITS,
};

use super::{OmpTranslator, ResourceMapper, TranslationError};
use crate::acp::bindings::omp::wire::{normalized_text, sanitized_command_output};

pub(super) type CommandSnapshot = (
    Option<SafeText<65536>>,
    BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
);

impl OmpTranslator {
    pub(super) fn command_snapshot(
        &mut self,
        id: &str,
    ) -> Result<CommandSnapshot, TranslationError> {
        Ok(self
            .live_turn()?
            .command_snapshots
            .get(id)
            .cloned()
            .unwrap_or((None, Default::default())))
    }

    pub(super) fn validate_command_snapshot(
        &mut self,
        id: &str,
        next: &CommandSnapshot,
    ) -> Result<(), TranslationError> {
        let active = self.live_turn()?;
        let old = active
            .command_snapshots
            .get(id)
            .map(command_snapshot_bytes)
            .unwrap_or(0);
        let total = active
            .command_snapshot_bytes
            .checked_sub(old)
            .and_then(|value| value.checked_add(command_snapshot_bytes(next)))
            .ok_or(TranslationError::Limit("command snapshot bytes"))?;
        (total <= LIMITS.wire.item_bytes_per_turn as usize)
            .then_some(())
            .ok_or(TranslationError::Limit("command snapshot bytes"))
    }

    pub(super) fn store_command_snapshot(
        &mut self,
        id: &str,
        next: CommandSnapshot,
    ) -> Result<(), TranslationError> {
        let active = self.live_turn()?;
        let old = active
            .command_snapshots
            .get(id)
            .map(command_snapshot_bytes)
            .unwrap_or(0);
        active.command_snapshot_bytes = active
            .command_snapshot_bytes
            .saturating_sub(old)
            .saturating_add(command_snapshot_bytes(&next));
        if next.0.is_none() && next.1.as_slice().is_empty() {
            active.command_snapshots.remove(id);
        } else {
            active.command_snapshots.insert(id.to_owned(), next);
        }
        Ok(())
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
    pub(super) fn is_omitted(&self) -> bool {
        self.output.is_none() && self.locations.is_none()
    }

    pub(super) fn apply(self, (output, locations): CommandSnapshot) -> CommandSnapshot {
        (
            self.output.unwrap_or(output),
            self.locations.unwrap_or(locations),
        )
    }
}

pub(super) fn command_update_payload(
    update: &Map<String, Value>,
    resources: &impl ResourceMapper,
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
    let locations = match update.get("locations") {
        None => None,
        Some(Value::Null) => Some(Default::default()),
        Some(Value::Array(locations)) => {
            if locations.len() > COMMAND_RESOURCE_LIMIT {
                return Err(TranslationError::Limit("command locations"));
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
            Some(BoundedVec::new("commandLocations", mapped)?)
        }
        Some(_) => return Err(TranslationError::MalformedFrame),
    };
    Ok(CommandPatch { output, locations })
}
