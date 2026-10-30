use super::{AcpService, BINDING_DIGEST_HEX, BINDING_ID, CharacterizationError};
use crate::acp::store::session_replace::ReplaceSessionCommand;

impl AcpService {
    pub(crate) fn transition_characterization_session(
        &self,
        command: ReplaceSessionCommand,
    ) -> Result<(), CharacterizationError> {
        if self.characterization.is_none()
            || command.start.binding_id.as_str() != BINDING_ID
            || command.start.binding_digest != BINDING_DIGEST_HEX
            || command.start.model_id.is_some()
        {
            return Err(CharacterizationError::Intent);
        }
        self.transition_admitted_session(command)
            .map_err(|_| CharacterizationError::Session)
    }
}
