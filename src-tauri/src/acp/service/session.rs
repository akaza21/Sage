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

pub(super) struct PreparedActivation {
    pub(super) owner: RuntimeOwner,
    pub(super) runtime: NativeRuntime,
    pub(super) intent: ActivationIntent,
    #[cfg(windows)]
    pub(super) browser_mcp_guard: Option<super::browser_mcp::BrowserMcpActivationGuard>,
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
    #[cfg(all(test, windows))]
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
        self.admit_session_start(&admitted, project_root, &mut command)?;
        self.session_starts
            .start_session(command)
            .await
            .map_err(Into::into)
    }

    pub(super) fn admit_session_start(
        &self,
        admitted: &crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        project_root: &crate::acp::bindings::registry::BindingProjectRoot,
        command: &mut StartSessionCommand,
    ) -> Result<(), CodexSessionError> {
        let identity = admitted.identity();
        if command.binding_id.as_str() != admitted.binding_id() {
            return Err(CodexSessionError::Intent);
        }
        project_root
            .revalidate()
            .map_err(|_| CodexSessionError::Intent)?;
        let model_discovery = admitted
            .has_model_discovery()
            .map_err(|error| CodexSessionError::Binding(error.into()))?;
        if model_discovery {
            self.codex_discovery
                .validate_session_model(
                    project_root
                        .codex_project_root()
                        .map_err(|_| CodexSessionError::Intent)?,
                    &command.binding_id,
                    identity.binding_digest,
                    command.model_id.as_ref(),
                )
                .map_err(|_| CodexSessionError::Intent)?;
        } else if command.model_id.is_some() {
            return Err(CodexSessionError::Intent);
        }
        command.binding_digest = identity.binding_digest_hex.to_owned();
        command.harness_version = Some(
            SafeText::new("harnessVersion", identity.harness_version)
                .map_err(|_| CodexSessionError::Intent)?,
        );
        Ok(())
    }

    pub(super) fn prepare_session_activation(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        mut command: ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<(ActivateSessionCommand, PreparedActivation), CodexSessionError> {
        self.prepare_session_activation_inner(admitted, &mut command, project_root)
            .map(|prepared| (command, prepared))
    }

    fn prepare_session_activation_inner(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: &mut ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<PreparedActivation, CodexSessionError> {
        let registered_binding_id = admitted.binding_id();
        let identity = admitted.identity();
        if !project_root.project_matches(&command.project_id) {
            return Err(CodexSessionError::Intent);
        }
        project_root
            .revalidate()
            .map_err(|_| CodexSessionError::Intent)?;
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let session = load_durable_session(&self.database, &owner)?;
        validate_session_identity(
            &session,
            registered_binding_id,
            identity.binding_digest_hex,
            identity.harness_version,
        )?;
        let model = session
            .model_id
            .map(|value| ModelId::new("modelId", value))
            .transpose()
            .map_err(|_| CodexSessionError::Intent)?;
        let native_root = project_root
            .native_root()
            .map_err(|_| CodexSessionError::Intent)?;
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
        command.artifact_digest = identity.artifact_digest.to_owned();
        let intent = ActivationIntent {
            binding_digest: identity.binding_digest,
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
        let harness_version = identity.harness_version.to_owned();
        #[cfg(windows)]
        let (mcp, browser_mcp_guard) = self.prepare_browser_mcp(&owner);
        #[cfg(windows)]
        if matches!(
            &project_root,
            crate::acp::bindings::registry::BindingProjectRoot::Codex(_)
        ) && ((browser_mcp_guard.is_some() && identity.harness_version != "0.146.0")
            || (self
                .browser_app
                .lock()
                .ok()
                .is_some_and(|slot| slot.is_some())
                && browser_mcp_guard.is_none()))
        {
            return Err(CodexSessionError::BrowserToolsUnavailable);
        }
        #[cfg(not(windows))]
        let mcp = crate::acp::mcp::McpAdmission::default();
        let runtime = admitted
            .into_factory(project_root, mcp)
            .construct(owner.runtime_epoch, owner.session_id.clone())
            .map_err(|_| CodexSessionError::Intent)?;
        Ok(PreparedActivation {
            owner,
            runtime,
            intent,
            #[cfg(windows)]
            browser_mcp_guard,
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

fn validate_session_identity(
    session: &DurableSession,
    binding_id: &str,
    binding_digest: &str,
    harness_version: &str,
) -> Result<(), CodexSessionError> {
    use crate::acp::bindings::continuation::{ContinuationCompatibility, classify};
    match classify(
        (
            &session.binding_id,
            &session.binding_digest,
            session.harness_version.as_deref(),
        ),
        (binding_id, binding_digest, harness_version),
    ) {
        ContinuationCompatibility::Compatible => Ok(()),
        ContinuationCompatibility::EarlierVersion
            if session.state == "ready" && session.external_session_id.is_some() =>
        {
            Err(CodexSessionError::EarlierCodexVersion)
        }
        ContinuationCompatibility::EarlierVersion | ContinuationCompatibility::Fresh => {
            Err(CodexSessionError::Intent)
        }
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;

    #[test]
    fn earlier_codex_thread_has_a_distinct_resume_refusal() {
        let session = DurableSession {
            binding_id: "codex-app-server".into(),
            binding_digest: "f8c2b995be263fe4bf00974043312673378081734ca8c98155d2a33887444b9e"
                .into(),
            harness_version: Some("0.145.0".into()),
            model_id: None,
            external_session_id: Some("thread-before-upgrade".into()),
            state: "ready".into(),
            residency: "cold".into(),
        };
        assert!(matches!(
            validate_session_identity(
                &session,
                "codex-app-server",
                "3bba7d86d2357d8fccd81bdba3d7f3f4278c80014d7e65e085b9a874330d7df4",
                "0.146.0",
            ),
            Err(CodexSessionError::EarlierCodexVersion)
        ));
        assert!(matches!(
            validate_session_identity(&session, "codex-app-server", "foreign-digest", "0.147.0",),
            Err(CodexSessionError::Intent)
        ));
    }
}
