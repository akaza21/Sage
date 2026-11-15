use super::*;

/// Each step is a no-op on a database already past it.
pub(super) fn apply(connection: &mut Connection) -> AppResult<()> {
    workspace_changes::apply(connection)?;
    title_provenance::apply(connection)?;
    session_scope::apply(connection)?;
    notifications::apply(connection)?;
    config_choices::apply(connection)?;
    session_resume_link::apply(connection)?;
    title_source::apply(connection)?;
    session_facts_turn_index::apply(connection)?;
    operation_terminal_index::apply(connection)?;
    turn_undo_intent_indexes::apply(connection)?;
    imported_history::apply(connection)?;
    session_capabilities::apply(connection)
}
