use crate::acp::runtime::adapter_pump::{PumpIntent, TurnScope};

pub(super) fn owner_bytes(intent: &PumpIntent) -> usize {
    intent.project_id.as_str().len()
        + intent.chat_id.as_str().len()
        + intent.session_id.as_str().len()
        + intent.active_turn.turn_id.as_str().len()
        + intent.external_session_id.as_str().len()
        + intent.binding_digest.len() * 2
}

pub(in crate::acp::runtime::adapter_store_journal) fn scope_heap_bytes(scope: &TurnScope) -> usize {
    scope.project_id.as_str().len()
        + scope.chat_id.as_str().len()
        + scope.session_id.as_str().len()
        + scope.turn_id.as_str().len()
        + scope.binding_digest.len()
}
