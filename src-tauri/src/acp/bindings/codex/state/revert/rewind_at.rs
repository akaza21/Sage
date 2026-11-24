//! Rewinding before any finished Turn: `thread/revert` names the Turn by the
//! native id Sage stored when Codex started it, so it holds across restarts
//! and consecutive rewinds. Codex drops that Turn and every later one; files
//! stay as they are.

use super::super::{AdapterError, CodexAdapter};
use crate::acp::bindings::codex::request::OutgoingRequest;

impl CodexAdapter {
    /// The cut always removes the latest Turn this runtime finished, so on
    /// success that Turn counts as reverted and the last-turn undo can no
    /// longer target it.
    pub(crate) fn rewind_at(
        &mut self,
        id: u64,
        native_turn: &str,
    ) -> Result<OutgoingRequest, AdapterError> {
        self.admit_revert()?;
        if native_turn.starts_with(super::super::REJECTED_TURN) {
            return Err(AdapterError::ForeignRequest);
        }
        let request = self.revert_before(id, native_turn)?;
        self.revert_target = self.previous_turn.as_ref().map(|turn| turn.id.as_str().to_owned());
        Ok(request)
    }
}
