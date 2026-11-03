use serde_json::Value;

use crate::acp::lifecycle::TurnState;

use super::CodexAdapter;

impl CodexAdapter {
    /// True only for a `turn/completed` notification that repeats the
    /// terminal this adapter last applied: the bound thread, the same turn id
    /// and the same terminal status, with no turn active or provisional since.
    /// Ingesting such a frame changes nothing; it is reported as a duplicate
    /// terminal. Every other frame, including a malformed one, is `false`.
    pub(crate) fn is_duplicate_terminal(&self, bytes: &[u8]) -> bool {
        if self.active_turn_id.is_some() || self.provisional_turn.is_some() {
            return false;
        }
        let Some(previous) = self.previous_turn.as_ref() else {
            return false;
        };
        let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
            return false;
        };
        let Some(object) = value.as_object() else {
            return false;
        };
        if object.contains_key("id")
            || object.get("method").and_then(Value::as_str) != Some("turn/completed")
        {
            return false;
        }
        let Some(params) = object.get("params").and_then(Value::as_object) else {
            return false;
        };
        let thread_bound = params
            .get("threadId")
            .and_then(Value::as_str)
            .is_some_and(|thread| self.expect_thread(thread).is_ok());
        let Some(turn) = params.get("turn").and_then(Value::as_object) else {
            return false;
        };
        let state = match turn.get("status").and_then(Value::as_str) {
            Some("completed") => TurnState::Completed,
            Some("interrupted") => TurnState::Interrupted,
            Some("failed") => TurnState::Failed,
            _ => return false,
        };
        thread_bound
            && turn.get("id").and_then(Value::as_str) == Some(previous.id.as_str())
            && state == previous.state
    }
}
