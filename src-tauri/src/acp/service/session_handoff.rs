use super::AcpService;
use crate::acp::{
    scheduler::RuntimeOwner,
    store::{PrepareTurnCommand, StoreError, context_handoff},
};

impl AcpService {
    pub(crate) fn freeze_handoff_prompt(
        &self,
        command: &mut PrepareTurnCommand,
    ) -> Result<(), StoreError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        // Codex model discovery can finish after activationReady. The channel
        // retains only the exact owner's admitted, confirmed current model.
        let model = self.lane_for(&owner).and_then(|lane| {
            lane.runtime.for_owner(&owner).ok()?;
            lane.runtime
                .config
                .snapshot(&owner)
                .filter(|snapshot| !snapshot.removed)
                .and_then(|snapshot| snapshot.config.confirmed_model)
        });
        context_handoff::freeze_prompt_with_model_context(
            &mut *self
                .database
                .write_connection()
                .map_err(|_| StoreError::Corrupt("storage recovery required"))?,
            command,
            model.as_ref(),
        )
    }
}
