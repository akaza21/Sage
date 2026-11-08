use super::super::super::state::ExternalKind;
use super::super::items::{file_change_edit_tool, normalize_file_change};
use super::super::{
    CodexAdapter, IncomingEvent, IngressDisposition, IngressError, ItemUpdateMode, ResourceMapper,
    normalize_item, normalize_materialized_command,
    support::{expect_thread_and_turn, identity, text},
};
use super::{child_thread, provisional};
use crate::acp::lifecycle::{ItemLifecycle, TerminalUpdate};
use serde_json::{Map, Value};

pub(super) fn item_event<M: ResourceMapper>(
    adapter: &mut CodexAdapter,
    params: &Map<String, Value>,
    requested: ItemLifecycle,
    mapper: &M,
    declarations: &crate::acp::declared_test::DeclaredTestCommands,
) -> Result<IngressDisposition, IngressError> {
    if adapter.has_provisional_turn() {
        return provisional::verify_first_user_message(adapter, params, requested);
    }
    let turn = expect_thread_and_turn(adapter, params)?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(IngressError::Malformed)?;
    let external = text(item, "id", 512)?;
    if adapter.turn_is_terminal(turn) && !adapter.item_is_terminal(turn, external) {
        return Ok(IngressDisposition::IgnoredLate);
    }
    let command = (item.get("type").and_then(Value::as_str) == Some("commandExecution"))
        .then(|| {
            adapter.materialize_command_item(
                turn,
                external,
                item,
                mapper,
                declarations,
                requested.is_terminal(),
            )
        })
        .transpose()?;
    let file_change =
        item.get("type").and_then(Value::as_str) == Some("fileChange") && requested.is_terminal();
    let (normalized, pair) = match command.as_ref() {
        Some(command) => (Some(normalize_materialized_command(command)?), None),
        None if file_change => {
            let (normalized, pair) = normalize_file_change(item, mapper, true)?;
            (Some(normalized), pair)
        }
        None => (normalize_item(item, requested, mapper)?, None),
    };
    // A settled file change is also an Edit tool row, ordered before its diff.
    let edit = file_change
        .then(|| file_change_edit_tool(item, mapper))
        .transpose()?
        .flatten()
        .map(|(payload, tool_state, terminal)| {
            Ok::<_, IngressError>(IncomingEvent::Item {
                id: adapter
                    .scoped_in_turn(ExternalKind::Item, turn, &format!("{external}:edit"))
                    .map_err(identity)?,
                lifecycle: terminal.unwrap_or(requested),
                tool_state,
                payload,
                update_mode: ItemUpdateMode::Replace,
            })
        })
        .transpose()?;
    let Some((mut payload, tool_state, terminal)) = normalized else {
        return Ok(IngressDisposition::Events(vec![]));
    };
    let lifecycle = terminal.unwrap_or(requested);
    if lifecycle.is_terminal() {
        match adapter
            .accept_item_terminal(turn, external, lifecycle)
            .map_err(identity)?
        {
            TerminalUpdate::NoOp => return Ok(IngressDisposition::DuplicateTerminal),
            TerminalUpdate::Conflict => return Err(IngressError::TerminalConflict),
            TerminalUpdate::Apply => {}
        }
    }
    if let Some(command) = command {
        if lifecycle.is_terminal() {
            adapter.remove_command_item(external);
        } else {
            adapter.commit_command_item(turn, external, command)?;
        }
    }
    let child_thread_id = child_thread::native_id(item, params)?;
    if let Some(child) = &child_thread_id {
        adapter.learn_bound_child(child.as_str());
    }
    adapter.enrich_subagent_payload(&mut payload);
    adapter.learn_child_threads(&payload);
    let id = adapter
        .scoped_in_turn(ExternalKind::Item, turn, external)
        .map_err(identity)?;
    let pair_event = pair.map(|pair| IncomingEvent::FilePair {
        id: id.clone(),
        old_text: pair.old_text,
        new_text: pair.new_text,
    });
    let event = match child_thread_id {
        Some(child_thread_id) => IncomingEvent::ChildThreadItem {
            id,
            lifecycle,
            payload,
            child_thread_id,
        },
        None => IncomingEvent::Item {
            id,
            lifecycle,
            tool_state,
            payload,
            update_mode: ItemUpdateMode::Replace,
        },
    };
    let mut events: Vec<_> = edit.into_iter().collect();
    events.push(event);
    events.extend(pair_event);
    Ok(IngressDisposition::Events(events))
}
