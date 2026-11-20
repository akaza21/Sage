use super::*;

impl CodexDiscoveryManager {
    pub(super) fn probe_admitted(
        &self,
        prepared: &mut PreparedCodexDiscovery,
        active: &ActiveCodexOperation,
        binding: Arc<AdmittedCodexBinding>,
        scope: CacheScope,
        probe: impl FnOnce(
            Arc<AdmittedCodexBinding>,
            CodexProjectRootAuthority,
            sage_acp_protocol::contract::RuntimeEpoch,
            CodexDiscoveryDeadline,
            &ActiveCodexOperation,
        ) -> Result<AdmittedDiscovery, CodexDiscoveryRunError>,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let operation_epoch = CodexOperationGate::prepared_epoch(
            prepared
                .operation
                .as_ref()
                .ok_or(CodexDiscoveryRunError::Cleanup)?,
        );
        let runtime_epoch = self.gate.allocate_runtime_epoch()?;
        let discovery = probe(
            binding,
            prepared
                .project_root
                .take()
                .ok_or(CodexDiscoveryRunError::Cleanup)?,
            runtime_epoch,
            prepared.deadline,
            active,
        )
        .inspect_err(|error| {
            if *error == CodexDiscoveryRunError::Drift {
                self.cache.clear();
                self.registration.invalidate_drift();
            }
        })?;
        prepared.deadline.ensure_total()?;
        discovery.revalidate().inspect_err(|_| self.cache.clear())?;
        let response = response_from(
            prepared.request.binding_id.clone(),
            scope.clone(),
            discovery.runtime_epoch(),
            &discovery.content,
        );
        prepared.deadline.ensure_total()?;
        self.publish(
            operation_epoch,
            prepared.catalog_generation,
            CacheKey {
                scope,
                runtime_epoch,
            },
            response.clone(),
        )?;
        Ok(response)
    }
}
