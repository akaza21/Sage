//! Transcript preparation belongs to the admitted binding and its profile.

use super::{AdmittedBinding, AdmittedBindingRecord};

impl AdmittedBindingRecord<'_> {
    /// Resolve a conversation rewind before the native undo can change anything.
    pub(crate) fn undo_cut(
        &self,
        session: &str,
        turn: &str,
    ) -> Result<Option<String>, crate::acp::bindings::claude::sdk::TranscriptError> {
        use crate::acp::bindings::claude::sdk::{TranscriptError, resolve_undo};
        match &self.binding {
            AdmittedBinding::ClaudeSdk(binding) => {
                let config = binding
                    .config_directory()
                    .ok_or(TranscriptError::Unreadable)?;
                resolve_undo(&config, session, turn)
            }
            _ => Ok(None),
        }
    }
}
