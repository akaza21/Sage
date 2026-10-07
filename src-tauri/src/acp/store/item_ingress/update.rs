use super::*;

pub(super) fn update_existing(
    transaction: &Transaction<'_>,
    command: &ItemIngressCommand,
    existing: ExistingItem,
    payload: String,
    bytes: i64,
    file_revision: bool,
) -> Result<ItemIngressMutation, StoreError> {
    if existing.project_id != command.project_id.as_str()
        || existing.chat_id != command.chat_id.as_str()
        || existing.turn_id != command.turn_id.as_str()
        || existing.activity_id.as_deref() != command.activity_id.as_ref().map(|v| v.as_str())
    {
        return Err(StoreError::Conflict {
            resource: "itemIdentity",
        });
    }
    let current = super::super::state::parse_item_lifecycle(&existing.lifecycle)?;
    let current_tool = existing
        .tool_state
        .as_deref()
        .map(super::super::state::parse_tool_state)
        .transpose()?;
    if current == command.lifecycle
        && current_tool == command.tool_state
        && existing.payload_digest == command.payload_digest
    {
        return duplicate(transaction, command, existing);
    }
    let replaceable_snapshot = current == command.lifecycle && file_revision;
    if current.is_terminal() && !replaceable_snapshot {
        return Err(StoreError::TerminalConflict { resource: "item" });
    }
    if current != command.lifecycle && !current.can_transition_to(command.lifecycle) {
        return Err(StoreError::InvalidTransition { resource: "item" });
    }
    if let (Some(old), Some(next)) = (current_tool, command.tool_state)
        && old != next
        && !old.can_transition_to(next)
    {
        return Err(StoreError::InvalidTransition { resource: "tool" });
    }
    let delta = bytes
        .checked_sub(existing.payload_bytes)
        .ok_or(StoreError::CounterExhausted)?;
    if delta > 0 {
        let _ = super::super::item_usage::ensure_capacity(transaction, command, 0, delta)?;
    }
    let next = entity_revision_from_i64(existing.revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let projection = next_projection_revision(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    let terminal = command
        .lifecycle
        .is_terminal()
        .then_some(command.now_millis);
    transaction.execute(
        "UPDATE items SET lifecycle=?2,tool_state=?3,payload_json=?4,payload_bytes=?5,
         payload_digest=?6,revision=?7,updated_at=?8,terminal_at=?9 WHERE id=?1",
        params![
            existing.item_id,
            item_lifecycle_text(command.lifecycle),
            command.tool_state.map(tool_state_text),
            payload,
            bytes,
            command.payload_digest,
            next.get() as i64,
            command.now_millis,
            terminal
        ],
    )?;
    super::super::item_usage::update(transaction, command, 0, delta)?;
    let item_id =
        SageItemId::parse(existing.item_id).map_err(|_| StoreError::Corrupt("item id"))?;
    super::super::item_usage::append_fact(transaction, command, &item_id, projection)?;
    publish_projection(
        transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    Ok(ItemIngressMutation {
        item_id,
        entity: EntityMutation {
            revision: next,
            projection_revision: projection,
            disposition: MutationDisposition::Applied,
        },
    })
}
