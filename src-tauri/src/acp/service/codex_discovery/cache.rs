use crate::acp::{
    bindings::codex::{
        CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryRunError,
        CodexProjectRootAuthority,
    },
    contract::{ProjectGeneration, RuntimeEpoch, SageProjectId, SageRootId},
    intent::BindingId,
    ipc::{
        CodexDiscoveryAccount, CodexDiscoveryModel, CodexDiscoveryResponse,
        GetCodexDiscoveryRequest,
    },
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct CacheScope {
    pub(super) project_id: SageProjectId,
    pub(super) project_generation: ProjectGeneration,
    pub(super) root_id: SageRootId,
    pub(super) root_identity: [u8; 24],
    pub(super) binding_digest: [u8; 32],
}

pub(super) fn validate_scope(
    request: &GetCodexDiscoveryRequest,
    root: &CodexProjectRootAuthority,
) -> Result<(), CodexDiscoveryRunError> {
    if root.project_id() != &request.project_id
        || root.project_generation() != request.project_generation
    {
        Err(CodexDiscoveryRunError::InvalidScope)
    } else {
        Ok(())
    }
}

pub(super) struct CacheKey {
    pub(super) scope: CacheScope,
    pub(super) runtime_epoch: RuntimeEpoch,
}

pub(super) struct CachedDiscovery {
    pub(super) key: CacheKey,
    pub(super) response: CodexDiscoveryResponse,
}

impl CachedDiscovery {
    pub(super) fn response_for(&self, scope: &CacheScope) -> Option<CodexDiscoveryResponse> {
        (self.key.scope == *scope && self.key.runtime_epoch == self.response.runtime_epoch)
            .then(|| self.response.clone())
    }
}

pub(super) fn response_from(
    binding_id: BindingId,
    scope: CacheScope,
    runtime_epoch: RuntimeEpoch,
    content: &CodexDiscoveryContent,
) -> CodexDiscoveryResponse {
    CodexDiscoveryResponse {
        project_id: scope.project_id,
        project_generation: scope.project_generation,
        root_id: scope.root_id,
        binding_id,
        runtime_epoch,
        account: match content.account {
            CodexAccountSnapshot::ManagedChatgpt => CodexDiscoveryAccount::ManagedChatgpt,
            CodexAccountSnapshot::SignedOut => CodexDiscoveryAccount::SignedOut,
        },
        models: content
            .models
            .iter()
            .map(|model| CodexDiscoveryModel {
                id: model.id.clone(),
                name: model.name.clone(),
                is_default: model.is_default,
            })
            .collect(),
        default_model_id: content.default_model_id.clone(),
    }
}
