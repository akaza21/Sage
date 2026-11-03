use crate::acp::{
    lifecycle::SessionState,
    process::LaunchFailure,
    runtime::{SessionTreeLease, ZeroProcessProof},
    scheduler::RuntimeOwner,
    store::FailActivationCommand,
};

use super::{AcpService, BINDING_DIGEST_HEX, CharacterizationError, support::map_slot};

impl AcpService {
    pub(super) async fn finish_launch_failure(
        &self,
        owner: RuntimeOwner,
        mut session: SessionTreeLease,
        artifact_digest: String,
        failure: LaunchFailure,
    ) -> Result<(), CharacterizationError> {
        let token = match failure.into_zero_process() {
            Ok(token) => token,
            Err(failure) => match failure.into_recovery() {
                Ok((_, recovery)) => match recovery.retry_zero() {
                    Ok(token) => token,
                    Err(_) => {
                        let _ = self.lane_at(0).runtime.mark_recovery(&owner);
                        return Err(CharacterizationError::ProcessRecovery);
                    }
                },
                Err(_) => {
                    let _ = self.lane_at(0).runtime.mark_recovery(&owner);
                    return Err(CharacterizationError::ProcessRecovery);
                }
            },
        };
        let proof = ZeroProcessProof::from_supervisor(owner.clone(), &token).map_err(|_| {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            CharacterizationError::ProcessRecovery
        })?;
        let fail_command = FailActivationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            binding_digest: BINDING_DIGEST_HEX.to_owned(),
            artifact_digest,
            terminal: SessionState::Failed,
            exit_kind: crate::acp::contract::RequiredText::new(
                "exitKind",
                "characterizationLaunchFailed",
            )
            .map_err(|_| CharacterizationError::Session)?,
            now_millis: crate::persistence::now_millis(),
        };
        if session
            .fail_activation_after_zero_process(fail_command, proof)
            .await
            .is_err()
        {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            return Err(CharacterizationError::Recovery);
        }
        self.lane_at(0)
            .runtime
            .abandon_start(&owner)
            .map_err(map_slot)?;
        Err(CharacterizationError::Launch)
    }
}
