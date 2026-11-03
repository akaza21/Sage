use super::*;

impl Lane<'_> {
    pub(super) fn recover_projection_authorities(
        &self,
        abort: &ActivationAbortIntent,
        journal: CoordinatorActivationJournal,
        token: crate::acp::process::ZeroProcessToken,
        host: Arc<RuntimeHost>,
        active: Option<ActiveCodexOperation>,
    ) -> Result<(), CodexSessionError> {
        if let Some(active) = active {
            active.settle(Err(
                crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
            ));
        }
        let _ = self.runtime.mark_recovery(&abort.owner);
        let lease = match journal.into_recovery_lease() {
            Ok(lease) => lease,
            Err(journal) => {
                self.retain_codex_recovery(PendingCodexRecovery::Journal {
                    abort: abort.clone(),
                    category: ActivationAbortCategory::RuntimeHandoffFailure,
                    journal: *journal,
                    proof: token,
                    host: Some((host, true)),
                });
                return Err(CodexSessionError::Recovery);
            }
        };
        let proof = match ZeroProcessProof::from_supervisor(abort.owner.clone(), &token) {
            Ok(proof) => proof,
            Err(_) => {
                self.retain_codex_recovery(PendingCodexRecovery::ProjectionToken {
                    owner: abort.owner.clone(),
                    lease,
                    token,
                    host,
                });
                return Err(CodexSessionError::Recovery);
            }
        };
        let _ = host.abandon_startup();
        self.retain_codex_recovery(PendingCodexRecovery::Projection {
            owner: abort.owner.clone(),
            lease,
            proof,
            host,
        });
        Err(CodexSessionError::Recovery)
    }
}
