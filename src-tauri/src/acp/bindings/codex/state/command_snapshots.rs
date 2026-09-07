use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::{
        incoming::ResourceMapper, state::command_output_stream::CommandOutputStream,
    },
    contract::BoundedVec,
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation},
    limits::LIMITS,
};

use super::{super::incoming::IngressError, CodexAdapter};

type CommandLocations = BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>;

/// The status is a parsed enum rather than retained wire JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
}

impl CommandStatus {
    pub(crate) fn is_terminal(self) -> bool {
        !matches!(self, Self::InProgress)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "inProgress",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Declined => "declined",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CommandSnapshot {
    stream: CommandOutputStream,
    location: Option<SafeResourceLocation>,
}

impl CommandSnapshot {
    fn retained_bytes(&self) -> usize {
        self.stream
            .retained_bytes()
            .saturating_add(location_bytes(self.location.as_ref()))
    }

    fn locations(&self) -> Result<CommandLocations, IngressError> {
        BoundedVec::new("commandLocations", self.location.iter().cloned().collect())
            .map_err(|_| IngressError::CollectionLimit)
    }

    fn output(&self) -> Result<Option<String>, IngressError> {
        let display = self.stream.render().map_err(|_| IngressError::Content)?;
        Ok((!display.is_empty()).then_some(display))
    }
}

/// Fully materialized, safe command data. It deliberately excludes command,
/// cwd, rawInput, rawOutput, action command, and action name.
pub(crate) struct MaterializedCommandItem {
    next: CommandSnapshot,
    pub(crate) status: CommandStatus,
    pub(crate) output: Option<String>,
    pub(crate) locations: CommandLocations,
}

#[derive(Debug, Default)]
pub(super) struct CommandItemSnapshots {
    values: BTreeMap<String, CommandSnapshot>,
    bytes: usize,
}

impl CommandItemSnapshots {
    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.bytes = 0;
    }

    pub(super) fn materialize<M: ResourceMapper>(
        &self,
        item_id: &str,
        item: &Map<String, Value>,
        mapper: &M,
        terminal: bool,
    ) -> Result<MaterializedCommandItem, IngressError> {
        let status = parse_status(item)?;
        let mut next = self.values.get(item_id).cloned().unwrap_or_default();

        if let Some(output) = item.get("aggregatedOutput") {
            match output {
                Value::String(value) => {
                    if terminal || status.is_terminal() {
                        next.stream
                            .replace_complete(value)
                            .map_err(|_| IngressError::Content)?;
                    } else {
                        next.stream
                            .replace_unfinished(value)
                            .map_err(|_| IngressError::Content)?;
                    }
                }
                Value::Null => next.stream.clear(),
                _ => return Err(IngressError::Malformed),
            }
        }
        // A nonterminal aggregate may end in a secret or path token. Only a
        // terminal item proves that flushing its pending suffix is safe.
        if terminal || status.is_terminal() {
            next.stream.finish().map_err(|_| IngressError::Content)?;
        }
        if let Some(actions) = item.get("commandActions") {
            next.location = parse_location(actions, mapper);
        }
        let output = next.output()?;
        let locations = next.locations()?;
        Ok(MaterializedCommandItem {
            next,
            status,
            output,
            locations,
        })
    }

    pub(super) fn append_delta(
        &self,
        item_id: &str,
        delta: &str,
    ) -> Result<MaterializedCommandItem, IngressError> {
        let mut next = self
            .values
            .get(item_id)
            .cloned()
            .ok_or(IngressError::Identity)?;
        next.stream
            .append(delta)
            .map_err(|_| IngressError::Content)?;
        let output = next.output()?;
        let locations = next.locations()?;
        Ok(MaterializedCommandItem {
            next,
            status: CommandStatus::InProgress,
            output,
            locations,
        })
    }

    pub(super) fn commit(
        &mut self,
        item_id: &str,
        materialized: MaterializedCommandItem,
    ) -> Result<(), IngressError> {
        let previous_bytes = self
            .values
            .get(item_id)
            .map_or(0, CommandSnapshot::retained_bytes);
        let next_bytes = materialized.next.retained_bytes();
        let total = self
            .bytes
            .checked_sub(previous_bytes)
            .and_then(|value| value.checked_add(next_bytes))
            .ok_or(IngressError::CollectionLimit)?;
        if total > LIMITS.wire.item_bytes_per_turn as usize
            || (!self.values.contains_key(item_id)
                && self.values.len() >= LIMITS.wire.items_per_turn as usize)
        {
            return Err(IngressError::CollectionLimit);
        }
        self.values.insert(item_id.to_owned(), materialized.next);
        self.bytes = total;
        Ok(())
    }

    pub(super) fn remove(&mut self, item_id: &str) {
        if let Some(snapshot) = self.values.remove(item_id) {
            self.bytes = self.bytes.saturating_sub(snapshot.retained_bytes());
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.values.len()
    }

    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.bytes
    }
}

impl CodexAdapter {
    pub(crate) fn materialize_command_item<M: ResourceMapper>(
        &self,
        turn: &str,
        item_id: &str,
        item: &Map<String, Value>,
        mapper: &M,
        terminal: bool,
    ) -> Result<MaterializedCommandItem, IngressError> {
        if !self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            return Err(IngressError::Identity);
        }
        self.command_item_snapshots
            .materialize(item_id, item, mapper, terminal)
    }

    pub(crate) fn commit_command_item(
        &mut self,
        turn: &str,
        item_id: &str,
        materialized: MaterializedCommandItem,
    ) -> Result<(), IngressError> {
        if !self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            return Err(IngressError::Identity);
        }
        self.command_item_snapshots.commit(item_id, materialized)
    }

    pub(crate) fn append_command_output_delta(
        &self,
        turn: &str,
        item_id: &str,
        delta: &str,
    ) -> Result<MaterializedCommandItem, IngressError> {
        if !self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            return Err(IngressError::Identity);
        }
        self.command_item_snapshots.append_delta(item_id, delta)
    }

    pub(crate) fn remove_command_item(&mut self, item_id: &str) {
        self.command_item_snapshots.remove(item_id);
    }

    pub(crate) fn clear_command_items(&mut self) {
        self.command_item_snapshots.clear();
    }
}

fn parse_status(item: &Map<String, Value>) -> Result<CommandStatus, IngressError> {
    match item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("inProgress")
    {
        "inProgress" => Ok(CommandStatus::InProgress),
        "completed" => Ok(CommandStatus::Completed),
        "failed" => Ok(CommandStatus::Failed),
        "declined" => Ok(CommandStatus::Declined),
        _ => Err(IngressError::Malformed),
    }
}

fn parse_location<M: ResourceMapper>(value: &Value, mapper: &M) -> Option<SafeResourceLocation> {
    let actions = value.as_array()?;
    let [action] = actions.as_slice() else {
        return None;
    };
    let action = action.as_object()?;
    let action_type = action.get("type")?.as_str()?;
    let path = action.get("path")?.as_str()?;
    // These mandatory typed-read fields are validated but intentionally never
    // retained or interpreted as an executable command/name.
    if action_type != "read"
        || !valid_required_text(action.get("command"))
        || !valid_required_text(action.get("name"))
        || !valid_path(path)
    {
        return None;
    }
    Some(match mapper.map_existing(path) {
        Ok(resource) => SafeResourceLocation::Resource { resource },
        Err(reason) => SafeResourceLocation::unavailable(reason),
    })
}

fn valid_required_text(value: Option<&Value>) -> bool {
    value.and_then(Value::as_str).is_some_and(|value| {
        !value.is_empty()
            && value.len() <= LIMITS.wire.frame_bytes as usize
            && !value.contains('\0')
    })
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= LIMITS.wire.absolute_path_bytes_native_only as usize
        && !path.contains('\0')
}

fn location_bytes(location: Option<&SafeResourceLocation>) -> usize {
    match location {
        Some(SafeResourceLocation::Resource { resource }) => resource
            .root_id
            .as_str()
            .len()
            .saturating_add(resource.relative_path.as_str().len()),
        Some(SafeResourceLocation::Unavailable { label, .. }) => label.as_str().len(),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::acp::{
        contract::SageRootId,
        intent::{SafeResourceRef, UnavailableResourceReason},
    };

    use super::*;

    struct Mapper;

    impl ResourceMapper for Mapper {
        fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
            SafeResourceRef::new(SageRootId::parse("fixture-root").unwrap(), "fixture.txt")
                .map_err(|_| UnavailableResourceReason::Unsupported)
        }
    }

    fn command(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn tracks_empty_commands_and_rejects_unknown_delta_state() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(json!({"status":"inProgress"}));
        let materialized = snapshots
            .materialize("command", &item, &Mapper, false)
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert!(matches!(
            snapshots.append_delta("unknown", "safe "),
            Err(IngressError::Identity)
        ));
    }

    #[test]
    fn static_and_delta_output_share_one_unflushed_stream() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(json!({"aggregatedOutput":"sk"}));
        let materialized = snapshots
            .materialize("command", &item, &Mapper, false)
            .unwrap();
        assert!(materialized.output.is_none());
        snapshots.commit("command", materialized).unwrap();
        let materialized = snapshots.append_delta("command", "-secret ").unwrap();
        assert_eq!(materialized.output.as_deref(), Some("[redacted] "));
    }

    #[test]
    fn terminal_replaces_and_flushes_but_null_clears() {
        let mut snapshots = CommandItemSnapshots::default();
        let initial = command(json!({"aggregatedOutput":"partial"}));
        let materialized = snapshots
            .materialize("command", &initial, &Mapper, false)
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        let terminal = command(json!({"status":"completed","aggregatedOutput":"final"}));
        let materialized = snapshots
            .materialize("command", &terminal, &Mapper, true)
            .unwrap();
        assert_eq!(materialized.output.as_deref(), Some("final"));

        snapshots.commit("command", materialized).unwrap();
        let omitted_terminal = command(json!({"status":"completed"}));
        let materialized = snapshots
            .materialize("command", &omitted_terminal, &Mapper, true)
            .unwrap();
        assert_eq!(materialized.output.as_deref(), Some("final"));

        let clear = command(json!({"status":"completed","aggregatedOutput":null}));
        let materialized = snapshots
            .materialize("command", &clear, &Mapper, true)
            .unwrap();
        assert!(materialized.output.is_none());
    }

    #[test]
    fn retains_only_typed_admitted_read_locations() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(
            json!({"commandActions":[{"type":"read","command":"cat","name":"read","path":"C:\\repo\\fixture.txt"}]}),
        );
        let materialized = snapshots
            .materialize("command", &item, &Mapper, false)
            .unwrap();
        assert_eq!(materialized.locations.as_slice().len(), 1);
        snapshots.commit("command", materialized).unwrap();
        let sparse = command(json!({}));
        let materialized = snapshots
            .materialize("command", &sparse, &Mapper, false)
            .unwrap();
        assert_eq!(materialized.locations.as_slice().len(), 1);
        let invalid =
            command(json!({"commandActions":[{"type":"read","path":"C:\\repo\\fixture.txt"}]}));
        let materialized = snapshots
            .materialize("command", &invalid, &Mapper, false)
            .unwrap();
        assert!(materialized.locations.as_slice().is_empty());
    }

    #[test]
    fn exact_count_and_turn_byte_limits_reject_plus_one_without_committing() {
        let mut snapshots = CommandItemSnapshots::default();
        let aggregate = "x ".repeat(8_192);
        for index in 0..LIMITS.wire.items_per_turn {
            let item = command(json!({"aggregatedOutput":aggregate}));
            let id = format!("command-{index}");
            let materialized = snapshots.materialize(&id, &item, &Mapper, false).unwrap();
            snapshots.commit(&id, materialized).unwrap();
        }
        assert_eq!(snapshots.len(), LIMITS.wire.items_per_turn as usize);
        assert_eq!(snapshots.bytes(), LIMITS.wire.item_bytes_per_turn as usize);

        let overflow = snapshots.append_delta("command-0", "x ").unwrap();
        assert!(matches!(
            snapshots.commit("command-0", overflow),
            Err(IngressError::CollectionLimit)
        ));
        assert_eq!(snapshots.bytes(), LIMITS.wire.item_bytes_per_turn as usize);

        let item = command(json!({}));
        let materialized = snapshots
            .materialize("command-overflow", &item, &Mapper, false)
            .unwrap();
        assert!(matches!(
            snapshots.commit("command-overflow", materialized),
            Err(IngressError::CollectionLimit)
        ));
        assert_eq!(snapshots.len(), LIMITS.wire.items_per_turn as usize);
    }
}
