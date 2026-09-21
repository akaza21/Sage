use std::path::Path;

use rusqlite::{OptionalExtension, params};

use crate::acp::{
    bindings::runtime::NativeRuntime,
    contract::SafeText,
    intent::{ExternalId, ModelId},
    runtime::{ActivationIntent, ActivationMode},
    scheduler::RuntimeOwner,
    store::{ActivateSessionCommand, SessionMutation, StartSessionCommand},
};

use super::{AcpService, CodexSessionError};

pub(super) struct PreparedCodexActivation {
    pub(super) owner: RuntimeOwner,
    pub(super) runtime: NativeRuntime,
    pub(super) intent: ActivationIntent,
    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    pub(super) harness_version: String,
}

struct DurableSession {
    binding_id: String,
    binding_digest: String,
    harness_version: Option<String>,
    model_id: Option<String>,
    external_session_id: Option<String>,
    state: String,
    residency: String,
}

impl AcpService {
    #[cfg(test)]
    pub(crate) async fn start_codex_session(
        &self,
        project_root: &crate::acp::bindings::codex::CodexProjectRootAuthority,
        command: StartSessionCommand,
    ) -> Result<SessionMutation, CodexSessionError> {
        let admitted = self
            .refresh_codex_binding()
            .map_err(CodexSessionError::Binding)?;
        self.start_session(
            admitted,
            &crate::acp::bindings::registry::BindingProjectRoot::Codex(
                project_root.clone_for_test(),
            ),
            command,
        )
        .await
    }

    pub(crate) async fn start_session(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        project_root: &crate::acp::bindings::registry::BindingProjectRoot,
        mut command: StartSessionCommand,
    ) -> Result<SessionMutation, CodexSessionError> {
        let binding = admitted.binding();
        if command.binding_id.as_str() != admitted.binding_id() {
            return Err(CodexSessionError::Intent);
        }
        self.codex_discovery
            .validate_session_model(
                project_root.codex(),
                &command.binding_id,
                binding.binding_digest(),
                command.model_id.as_ref(),
            )
            .map_err(|_| CodexSessionError::Intent)?;
        command.binding_digest = binding.binding_digest_hex().to_owned();
        command.harness_version = Some(
            SafeText::new("harnessVersion", binding.harness_version())
                .map_err(|_| CodexSessionError::Intent)?,
        );
        self.sessions
            .start_session(command)
            .await
            .map_err(Into::into)
    }

    pub(super) fn prepare_session_activation(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        mut command: ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<(ActivateSessionCommand, PreparedCodexActivation), CodexSessionError> {
        self.prepare_session_activation_inner(admitted, &mut command, project_root)
            .map(|prepared| (command, prepared))
    }

    fn prepare_session_activation_inner(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: &mut ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<PreparedCodexActivation, CodexSessionError> {
        let registered_binding_id = admitted.binding_id();
        let binding = admitted.binding();
        let codex_root = project_root.codex();
        if codex_root.project_id() != &command.project_id {
            return Err(CodexSessionError::Intent);
        }
        codex_root
            .revalidate()
            .map_err(|_| CodexSessionError::Intent)?;
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let session = load_durable_session(&self.database, &owner)?;
        if session.binding_id != registered_binding_id
            || session.binding_digest != binding.binding_digest_hex()
            || session.harness_version.as_deref() != Some(binding.harness_version())
        {
            return Err(CodexSessionError::Intent);
        }
        let model = session
            .model_id
            .map(|value| ModelId::new("modelId", value))
            .transpose()
            .map_err(|_| CodexSessionError::Intent)?;
        let native_root = native_app_server_root(codex_root.as_path())?;
        let mode = match (
            session.state.as_str(),
            session.residency.as_str(),
            session.external_session_id,
        ) {
            ("created", "cold", None) => ActivationMode::New { native_root, model },
            ("ready", "cold", Some(external)) => ActivationMode::Resume {
                external_session_id: ExternalId::new("externalSessionId", external)
                    .map_err(|_| CodexSessionError::Intent)?,
                native_root,
                model,
            },
            _ => return Err(CodexSessionError::Intent),
        };
        let ceiling = admitted
            .supported_capabilities()
            .map_err(|error| CodexSessionError::Binding(error.into()))?;
        command.artifact_digest = binding.artifact_digest().to_owned();
        let intent = ActivationIntent {
            binding_digest: binding.binding_digest(),
            artifact_digest: command.artifact_digest.clone(),
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            mode,
            curated_ceiling: ceiling.clone(),
            sage_policy: ceiling,
        };
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        let harness_version = binding.harness_version().to_owned();
        let runtime = admitted
            .into_factory(project_root)
            .construct(owner.runtime_epoch, owner.session_id.clone())
            .map_err(|_| CodexSessionError::Intent)?;
        Ok(PreparedCodexActivation {
            owner,
            runtime,
            intent,
            #[cfg(any(debug_assertions, feature = "diagnostics"))]
            harness_version,
        })
    }
}

fn load_durable_session(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
) -> Result<DurableSession, CodexSessionError> {
    database
        .connection()
        .query_row(
            "SELECT binding_id,binding_digest,harness_version,model_id,external_session_id,state,residency
             FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",
            params![owner.project_id.as_str(), owner.chat_id.as_str(), owner.session_id.as_str()],
            |row| Ok(DurableSession {
                binding_id: row.get(0)?,
                binding_digest: row.get(1)?,
                harness_version: row.get(2)?,
                model_id: row.get(3)?,
                external_session_id: row.get(4)?,
                state: row.get(5)?,
                residency: row.get(6)?,
            }),
        )
        .optional()
        .map_err(|_| CodexSessionError::Intent)?
        .ok_or(CodexSessionError::Intent)
}

fn native_app_server_root(path: &Path) -> Result<String, CodexSessionError> {
    let text = path.to_str().ok_or(CodexSessionError::Intent)?;
    let text = text
        .strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix(r"//?/"))
        .unwrap_or(text);
    if text.is_empty() {
        return Err(CodexSessionError::Intent);
    }
    Ok(text.to_owned())
}
