use sha2::{Digest, Sha256};

use super::{AdapterError, CodexAdapter, LIMITS};

impl CodexAdapter {
    pub(in crate::acp::bindings::codex) fn remember_unretried_error(
        &mut self,
        turn: &str,
        message: &str,
    ) -> Result<(), AdapterError> {
        let fingerprints = if self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            &mut self.active_unretried_error_fingerprints
        } else {
            &mut self
                .previous_turn
                .as_mut()
                .filter(|previous| previous.id.as_str() == turn)
                .ok_or(AdapterError::ForeignIdentity)?
                .unretried_error_fingerprints
        };
        let fingerprint: [u8; 32] = Sha256::digest(message.as_bytes()).into();
        if fingerprints.contains(&fingerprint) {
            return Ok(());
        }
        if fingerprints.len() >= LIMITS.wire.items_per_turn as usize {
            return Ok(());
        }
        fingerprints.push(fingerprint);
        Ok(())
    }

    pub(in crate::acp::bindings::codex) fn has_unretried_error(
        &self,
        turn: &str,
        message: &str,
    ) -> bool {
        let fingerprints = if self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            Some(&self.active_unretried_error_fingerprints)
        } else {
            self.previous_turn
                .as_ref()
                .filter(|previous| previous.id.as_str() == turn)
                .map(|previous| &previous.unretried_error_fingerprints)
        };
        let fingerprint: [u8; 32] = Sha256::digest(message.as_bytes()).into();
        fingerprints.is_some_and(|fingerprints| fingerprints.contains(&fingerprint))
    }
}
