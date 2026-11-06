use crate::acp::intent::BindingId;

use super::{AcpService, SessionDispatchError};

impl AcpService {
    pub(crate) fn dispatch_session_close(
        &self,
        binding_id: &BindingId,
        command: crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, SessionDispatchError> {
        let binding = self
            .session_operation_binding(binding_id)
            .map_err(|_| SessionDispatchError::Binding)?;
        self.file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_chat(command.project_id.as_str(), command.chat_id.as_str());
        self.revoke_browser_chat(&command.project_id, &command.chat_id);
        binding
            .dispatch_close(self, command)
            .map_err(SessionDispatchError::Operation)
    }
}
