use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::acp::{
    bindings::codex::{
        incoming::{
            ResourceMapper,
            support::{command_cwd, command_exit_code, sanitized_command_line},
        },
        state::command_output_stream::CommandOutputStream,
    },
    contract::BoundedVec,
    declared_test::{CommandActivity, DeclaredTestCommands},
    intent::{COMMAND_RESOURCE_LIMIT, SafeResourceLocation},
    limits::LIMITS,
};

use super::{
    super::incoming::IngressError,
    CodexAdapter,
    command_explore::{CommandExplore, classify},
};

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

#[derive(Debug, Clone, Default, PartialEq)]
struct CommandSnapshot {
    stream: CommandOutputStream,
    command: Option<String>,
    location: Option<SafeResourceLocation>,
    cwd: Option<SafeResourceLocation>,
    exit_code: Option<i32>,
    activity: CommandActivity,
    explore: Option<CommandExplore>,
}

impl CommandSnapshot {
    fn retained_bytes(&self) -> usize {
        self.stream
            .retained_bytes()
            .saturating_add(
                self.explore
                    .as_ref()
                    .map_or(0, CommandExplore::retained_bytes),
            )
            .saturating_add(self.command.as_ref().map_or(0, String::len))
            .saturating_add(location_bytes(self.location.as_ref()))
            .saturating_add(location_bytes(self.cwd.as_ref()))
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

pub(crate) struct MaterializedCommandItem {
    next: CommandSnapshot,
    pub(crate) status: CommandStatus,
    pub(crate) command: Option<String>,
    pub(crate) output: Option<String>,
    pub(crate) locations: CommandLocations,
    pub(crate) cwd: Option<SafeResourceLocation>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) activity: CommandActivity,
    pub(crate) explore: Option<CommandExplore>,
}

#[derive(Clone, Debug, Default, PartialEq)]
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
        declarations: &DeclaredTestCommands,
        terminal: bool,
    ) -> Result<MaterializedCommandItem, IngressError> {
        let status = parse_status(item)?;
        let mut next = self.values.get(item_id).cloned().unwrap_or_default();
        if let Some(command) = raw_command(item) {
            next.activity = declarations.activity_for(command);
            next.command = Some(sanitized_command_line(command)?).filter(|value| !value.is_empty());
        } else if matches!(item.get("command"), Some(Value::Null)) {
            next.command = None;
        }

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
            next.explore = classify(actions);
        }
        if item.contains_key("cwd") {
            next.cwd = command_cwd(item, mapper);
        }
        if item.contains_key("exitCode") {
            next.exit_code = command_exit_code(item);
        }
        let output = next.output()?;
        let locations = next.locations()?;
        let activity = next.activity;
        let command = next.command.clone();
        let cwd = next.cwd.clone();
        let exit_code = next.exit_code;
        let explore = next.explore.clone();
        Ok(MaterializedCommandItem {
            next,
            status,
            command,
            output,
            locations,
            cwd,
            exit_code,
            activity,
            explore,
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
        let command = next.command.clone();
        let cwd = next.cwd.clone();
        let exit_code = next.exit_code;
        Ok(MaterializedCommandItem {
            activity: next.activity,
            explore: next.explore.clone(),
            next,
            status: CommandStatus::InProgress,
            command,
            output,
            locations,
            cwd,
            exit_code,
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
        declarations: &DeclaredTestCommands,
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
            .materialize(item_id, item, mapper, declarations, terminal)
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

fn raw_command(item: &Map<String, Value>) -> Option<&str> {
    item.get("command").and_then(Value::as_str).filter(|value| {
        // The enclosing frame already has the wire byte cap. An empty or NUL
        // command and any non-string shape simply lacks a stable admitted raw
        // command contract, so it remains an ordinary command rather than
        // tearing down the session.
        !value.is_empty() && !value.contains('\0')
    })
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
        declared_test::declared_test_commands_for_project,
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
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert!(matches!(
            snapshots.append_delta("unknown", "safe "),
            Err(IngressError::Identity)
        ));
    }

    #[test]
    fn a_command_cwd_is_retained_as_a_project_relative_resource() {
        let snapshots = CommandItemSnapshots::default();
        let item = command(json!({
            "status":"completed",
            "cwd":"/workspace/fixture/nested",
            "exitCode":3
        }));
        let materialized = snapshots
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                true,
            )
            .unwrap();
        let Some(SafeResourceLocation::Resource { resource }) = materialized.cwd else {
            panic!("expected a mapped cwd resource");
        };
        assert_eq!(resource.root_id.as_str(), "fixture-root");
        assert_eq!(resource.relative_path.as_str(), "fixture.txt");
        assert_eq!(materialized.exit_code, Some(3));
    }

    #[test]
    fn an_explicit_null_cwd_and_exit_code_clear_the_retained_execution() {
        let mut snapshots = CommandItemSnapshots::default();
        let started = command(json!({
            "status":"inProgress",
            "cwd":"/workspace/fixture",
            "exitCode":0
        }));
        let materialized = snapshots
            .materialize(
                "command",
                &started,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        let cleared = command(json!({"status":"completed","cwd":null,"exitCode":null}));
        let materialized = snapshots
            .materialize(
                "command",
                &cleared,
                &Mapper,
                &DeclaredTestCommands::default(),
                true,
            )
            .unwrap();
        assert!(materialized.cwd.is_none());
        assert!(materialized.exit_code.is_none());
    }

    #[test]
    fn recognizes_a_declared_test_and_retains_only_the_sanitized_command() {
        let root =
            std::env::temp_dir().join(format!("sage-command-activity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"scripts":{"test":"vitest"}}"#,
        )
        .unwrap();
        let declarations = declared_test_commands_for_project(root.as_path());
        let mut snapshots = CommandItemSnapshots::default();
        let declared = command(json!({"command":"vitest"}));
        let materialized = snapshots
            .materialize("command", &declared, &Mapper, &declarations, false)
            .unwrap();
        assert_eq!(materialized.activity, CommandActivity::DeclaredTest);
        assert_eq!(materialized.command.as_deref(), Some("vitest"));
        snapshots.commit("command", materialized).unwrap();

        let near_miss = command(json!({"command":"vitest --coverage"}));
        let materialized = snapshots
            .materialize("command", &near_miss, &Mapper, &declarations, false)
            .unwrap();
        assert_eq!(materialized.activity, CommandActivity::Ordinary);
        snapshots.commit("near-miss", materialized).unwrap();

        let stored = snapshots.values.get("near-miss").unwrap();
        assert_eq!(stored.activity, CommandActivity::Ordinary);
        assert_eq!(stored.command.as_deref(), Some("vitest --coverage"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_retained_command_is_redacted_and_flattened_to_one_line() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(json!({
            "command":"curl -H \"Authorization: Bearer sk-live-super-secret\"\n\tC:\\private\\run.exe"
        }));
        let materialized = snapshots
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        let retained = materialized.command.clone().unwrap();
        assert!(!retained.contains("sk-live-super-secret"));
        assert!(!retained.contains("private"));
        assert!(retained.contains("[redacted]"));
        assert!(retained.contains("[path]"));
        assert!(!retained.contains('\n') && !retained.contains('\t'));
        snapshots.commit("command", materialized).unwrap();
        assert!(!format!("{snapshots:?}").contains("sk-live-super-secret"));
    }

    #[test]
    fn an_explicit_null_command_clears_the_retained_command() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(json!({"command":"cargo test"}));
        let materialized = snapshots
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        let sparse = command(json!({}));
        let materialized = snapshots
            .materialize(
                "command",
                &sparse,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        assert_eq!(materialized.command.as_deref(), Some("cargo test"));
        snapshots.commit("command", materialized).unwrap();
        let cleared = command(json!({"command":null}));
        let materialized = snapshots
            .materialize(
                "command",
                &cleared,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        assert!(materialized.command.is_none());
    }

    #[test]
    fn unusable_raw_command_shapes_stay_ordinary_without_teardown() {
        let snapshots = CommandItemSnapshots::default();
        for value in [
            json!(""),
            json!(format!("{}npm test", '\0')),
            json!(["npm", "test"]),
        ] {
            let item = command(json!({"command":value}));
            assert_eq!(
                snapshots
                    .materialize(
                        "command",
                        &item,
                        &Mapper,
                        &DeclaredTestCommands::default(),
                        false,
                    )
                    .unwrap()
                    .activity,
                CommandActivity::Ordinary
            );
        }
    }

    #[test]
    fn static_and_delta_output_share_one_unflushed_stream() {
        let mut snapshots = CommandItemSnapshots::default();
        let item = command(json!({"aggregatedOutput":"sk"}));
        let materialized = snapshots
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
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
            .materialize(
                "command",
                &initial,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        snapshots.commit("command", materialized).unwrap();
        let terminal = command(json!({"status":"completed","aggregatedOutput":"final"}));
        let materialized = snapshots
            .materialize(
                "command",
                &terminal,
                &Mapper,
                &DeclaredTestCommands::default(),
                true,
            )
            .unwrap();
        assert_eq!(materialized.output.as_deref(), Some("final"));

        snapshots.commit("command", materialized).unwrap();
        let omitted_terminal = command(json!({"status":"completed"}));
        let materialized = snapshots
            .materialize(
                "command",
                &omitted_terminal,
                &Mapper,
                &DeclaredTestCommands::default(),
                true,
            )
            .unwrap();
        assert_eq!(materialized.output.as_deref(), Some("final"));

        let clear = command(json!({"status":"completed","aggregatedOutput":null}));
        let materialized = snapshots
            .materialize(
                "command",
                &clear,
                &Mapper,
                &DeclaredTestCommands::default(),
                true,
            )
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
            .materialize(
                "command",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        assert_eq!(materialized.locations.as_slice().len(), 1);
        snapshots.commit("command", materialized).unwrap();
        let sparse = command(json!({}));
        let materialized = snapshots
            .materialize(
                "command",
                &sparse,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        assert_eq!(materialized.locations.as_slice().len(), 1);
        let invalid =
            command(json!({"commandActions":[{"type":"read","path":"C:\\repo\\fixture.txt"}]}));
        let materialized = snapshots
            .materialize(
                "command",
                &invalid,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
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
            let materialized = snapshots
                .materialize(&id, &item, &Mapper, &DeclaredTestCommands::default(), false)
                .unwrap();
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
            .materialize(
                "command-overflow",
                &item,
                &Mapper,
                &DeclaredTestCommands::default(),
                false,
            )
            .unwrap();
        assert!(matches!(
            snapshots.commit("command-overflow", materialized),
            Err(IngressError::CollectionLimit)
        ));
        assert_eq!(snapshots.len(), LIMITS.wire.items_per_turn as usize);
    }
}
