use std::sync::Mutex;

use crate::acp::{
    bindings::codex::{
        CodexAccountSnapshot, CodexDiscoveryContent, CodexDiscoveryRunError,
        CodexProjectRootAuthority,
    },
    contract::{ProjectGeneration, RuntimeEpoch, SageProjectId, SageRootId},
    intent::{BindingId, ModelId},
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

pub(super) struct DiscoveryCache {
    entry: Mutex<Option<CachedDiscovery>>,
}

pub(super) struct CachedDiscovery {
    pub(super) key: CacheKey,
    pub(super) response: CodexDiscoveryResponse,
}

impl DiscoveryCache {
    pub(super) fn new() -> Self {
        Self {
            entry: Mutex::new(None),
        }
    }

    pub(super) fn clear(&self) {
        match self.entry.lock() {
            Ok(mut entry) => *entry = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }

    pub(super) fn response_for(
        &self,
        scope: &CacheScope,
    ) -> Result<Option<CodexDiscoveryResponse>, CodexDiscoveryRunError> {
        let entry = self
            .entry
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        Ok(entry.as_ref().and_then(|entry| entry.response_for(scope)))
    }

    pub(super) fn validate_session_model(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_id: &BindingId,
        binding_digest: [u8; 32],
        model_id: Option<&ModelId>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let entry = self
            .entry
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        entry
            .as_ref()
            .ok_or(CodexDiscoveryRunError::InvalidScope)?
            .validate_session_model(project_root, binding_id, binding_digest, model_id)
    }

    pub(super) fn publish_if_current(
        &self,
        key: CacheKey,
        response: CodexDiscoveryResponse,
        is_current: impl FnOnce() -> bool,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut entry = self
            .entry
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        if !is_current() {
            return Err(CodexDiscoveryRunError::Drift);
        }
        *entry = Some(CachedDiscovery { key, response });
        Ok(())
    }

    #[cfg(all(test, windows))]
    pub(super) fn seed_session_validation(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_digest: [u8; 32],
        binding_id: &str,
        model: Option<ModelId>,
    ) {
        use crate::acp::{
            contract::{RequiredText, RuntimeEpoch},
            ipc::{CodexDiscoveryAccount, CodexDiscoveryModel},
        };

        let scope = CacheScope {
            project_id: project_root.project_id().clone(),
            project_generation: project_root.project_generation(),
            root_id: project_root.root_id().clone(),
            root_identity: project_root.identity_bytes(),
            binding_digest,
        };
        let runtime_epoch = RuntimeEpoch::new(97).unwrap();
        let models = model
            .map(|id| {
                vec![CodexDiscoveryModel {
                    id,
                    name: RequiredText::new("modelName", "Codex test model").unwrap(),
                    is_default: true,
                }]
            })
            .unwrap_or_default();
        *self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CachedDiscovery {
            key: CacheKey {
                scope: scope.clone(),
                runtime_epoch,
            },
            response: CodexDiscoveryResponse {
                project_id: project_root.project_id().clone(),
                project_generation: project_root.project_generation(),
                root_id: scope.root_id,
                binding_id: BindingId::new("bindingId", binding_id).unwrap(),
                runtime_epoch,
                account: CodexDiscoveryAccount::ManagedChatgpt,
                models,
                default_model_id: None,
            },
        });
    }
}

impl CachedDiscovery {
    pub(super) fn response_for(&self, scope: &CacheScope) -> Option<CodexDiscoveryResponse> {
        (self.key.scope.same_catalog(scope)
            && self.key.runtime_epoch == self.response.runtime_epoch)
            .then(|| CodexDiscoveryResponse {
                project_generation: scope.project_generation,
                ..self.response.clone()
            })
    }

    pub(super) fn validate_session_model(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_id: &BindingId,
        binding_digest: [u8; 32],
        model_id: Option<&ModelId>,
    ) -> Result<(), CodexDiscoveryRunError> {
        if !self.key.scope.matches_session(project_root, binding_digest) {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        if &self.response.binding_id != binding_id
            || !matches!(
                self.response.account,
                CodexDiscoveryAccount::ManagedChatgpt | CodexDiscoveryAccount::ProviderManaged
            )
        {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        if model_id
            .is_some_and(|model| !self.response.models.iter().any(|entry| &entry.id == model))
        {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        Ok(())
    }
}

impl CacheScope {
    fn same_catalog(&self, other: &Self) -> bool {
        self.project_id == other.project_id
            && self.root_id == other.root_id
            && self.root_identity == other.root_identity
            && self.binding_digest == other.binding_digest
    }

    fn matches_session(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_digest: [u8; 32],
    ) -> bool {
        self.project_id == *project_root.project_id()
            && self.root_id == *project_root.root_id()
            && self.root_identity == project_root.identity_bytes()
            && self.binding_digest == binding_digest
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
            CodexAccountSnapshot::ProviderManaged => CodexDiscoveryAccount::ProviderManaged,
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
