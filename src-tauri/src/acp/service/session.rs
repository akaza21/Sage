use rusqlite::{OptionalExtension, params};

use crate::acp::{
    bindings::runtime::NativeRuntime,
    contract::{SafeText, SageTurnId},
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
    /// The native thread of the earlier session this one resumes, if any.
    resumed_thread: Option<String>,
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
        } else if command
            .model_id
            .as_ref()
            .is_some_and(|model| !admitted.accepts_launch_model(model.as_str()))
        {
            return Err(CodexSessionError::Intent);
        }
        command.binding_digest = identity.binding_digest_hex.to_owned();
        // A user-defined agent reports no version; its session stores none,
        // as the schema admits only a non-empty version.
        command.harness_version = (!identity.harness_version.is_empty())
            .then(|| SafeText::new("harnessVersion", identity.harness_version))
            .transpose()
            .map_err(|_| CodexSessionError::Intent)?;
        self.session_starts
            .start_session(command)
            .await
            .map_err(Into::into)
    }

    pub(super) fn prepare_session_activation(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        mut command: ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<(ActivateSessionCommand, PreparedActivation), CodexSessionError> {
        self.prepare_session_activation_inner(
            admitted,
            &mut command,
            project_root,
            #[cfg(windows)]
            |owner| self.prepare_browser_mcp(owner),
        )
        .map(|prepared| (command, prepared))
    }

    fn prepare_session_activation_inner(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: &mut ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
        #[cfg(windows)] prepare_browser: impl FnOnce(
            &RuntimeOwner,
        ) -> (
            crate::acp::mcp::McpAdmission,
            Option<super::browser_mcp::BrowserMcpActivationGuard>,
        ),
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
        let session = load_durable_session(
            &self.database,
            &owner.project_id,
            &owner.chat_id,
            &owner.session_id,
        )?;
        validate_session_identity(
            &session,
            registered_binding_id,
            identity.binding_digest_hex,
            identity.harness_version,
        )?;
        if session.binding_digest != identity.binding_digest_hex {
            rebind_claude_sdk_session(
                &self.database,
                &owner,
                identity.binding_digest_hex,
                identity.harness_version,
            )?;
        }
        let model = session
            .model_id
            .clone()
            .map(|value| ModelId::new("modelId", value))
            .transpose()
            .map_err(|_| CodexSessionError::Intent)?;
        let native_root = project_root
            .native_root()
            .map_err(|_| CodexSessionError::Intent)?;
        let rewind_before = pending_cut(&self.database, &owner)?;
        let mode = activation_mode(session, native_root, model, rewind_before)?;
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
        let (mut mcp, mut browser_mcp_guard) = prepare_browser(&owner);
        #[cfg(windows)]
        {
            let browser_available = browser_mcp_guard.is_some()
                || self
                    .browser_app
                    .lock()
                    .ok()
                    .is_some_and(|slot| slot.is_some());
            if browser_available {
                let notice = admitted.browser_mcp_version_notice().or_else(|| {
                    browser_mcp_guard.is_none().then_some(
                        "Browser tools are unavailable for this chat because the browser helper could not be prepared. You can continue chatting.",
                    )
                });
                if let Some(notice) = notice {
                    mcp = crate::acp::mcp::McpAdmission::without_browser_tools(notice);
                    // Revocation must happen before constructing the normal runtime.
                    drop(browser_mcp_guard.take());
                }
            }
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
    project_id: &crate::acp::contract::SageProjectId,
    chat_id: &crate::acp::contract::SageChatId,
    session_id: &crate::acp::contract::SageAgentSessionId,
) -> Result<DurableSession, CodexSessionError> {
    database
        .connection()
        .query_row(
            "SELECT s.binding_id,s.binding_digest,s.harness_version,s.model_id,
                    s.external_session_id,s.state,s.residency,earlier.external_session_id
             FROM agent_sessions s
             LEFT JOIN agent_sessions earlier ON earlier.id=s.resumes_session_id
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3",
            params![project_id.as_str(), chat_id.as_str(), session_id.as_str()],
            |row| {
                Ok(DurableSession {
                    binding_id: row.get(0)?,
                    binding_digest: row.get(1)?,
                    harness_version: row.get(2)?,
                    model_id: row.get(3)?,
                    external_session_id: row.get(4)?,
                    state: row.get(5)?,
                    residency: row.get(6)?,
                    resumed_thread: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|_| CodexSessionError::Intent)?
        .ok_or(CodexSessionError::Intent)
}

/// The undone Turn a resumed native thread is cut before, while one waits.
/// A pending restart never reaches here: the head is replaced first.
fn pending_cut(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
) -> Result<Option<SageTurnId>, CodexSessionError> {
    let rewind =
        crate::acp::store::pending_thread_rewind(&database.connection(), owner.session_id.as_str())
            .map_err(|_| CodexSessionError::Intent)?;
    Ok(rewind
        .filter(|rewind| rewind.thread == crate::acp::store::UndoneThread::Cut)
        .map(|rewind| rewind.turn_id))
}

fn activation_mode(
    session: DurableSession,
    native_root: String,
    model: Option<ModelId>,
    rewind_before: Option<SageTurnId>,
) -> Result<ActivationMode, CodexSessionError> {
    let thread = |external: String| {
        ExternalId::new("externalSessionId", external).map_err(|_| CodexSessionError::Intent)
    };
    Ok(
        match (
            session.state.as_str(),
            session.residency.as_str(),
            session.external_session_id,
            session.resumed_thread,
        ) {
            ("created", "cold", None, None) => ActivationMode::New { native_root, model },
            // A session that continues an earlier one resumes its thread; the
            // first activation records that thread as this session's own.
            ("created", "cold", None, Some(external)) => ActivationMode::Resume {
                external_session_id: thread(external)?,
                native_root,
                model,
                rewind_before: None,
            },
            // A parked session resumes its own thread, cut before a Turn the
            // user undid while one waits.
            ("ready", "cold", Some(external), _) => ActivationMode::Resume {
                external_session_id: thread(external)?,
                native_root,
                model,
                rewind_before,
            },
            _ => return Err(CodexSessionError::Intent),
        },
    )
}

/// Moves a cold Claude Code SDK session to the admitted CLI after an
/// update; validation admits no other binding whose digest differs.
fn rebind_claude_sdk_session(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
    binding_digest: &str,
    harness_version: &str,
) -> Result<(), CodexSessionError> {
    let updated = database
        .connection()
        .execute(
            "UPDATE agent_sessions SET binding_digest=?4,harness_version=?5
             WHERE project_id=?1 AND chat_id=?2 AND id=?3 AND binding_id=?6
               AND state IN ('created','ready') AND residency='cold'",
            params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                binding_digest,
                harness_version,
                crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID,
            ],
        )
        .map_err(|_| CodexSessionError::Intent)?;
    (updated == 1)
        .then_some(())
        .ok_or(CodexSessionError::Intent)
}

fn validate_session_identity(
    session: &DurableSession,
    binding_id: &str,
    binding_digest: &str,
    harness_version: &str,
) -> Result<(), CodexSessionError> {
    // Claude Code keeps its own sessions across updates, and admission
    // already pins the protocol line, so a resumable chat follows the CLI.
    if binding_id == crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID
        && session.binding_id == binding_id
    {
        return Ok(());
    }
    if session.binding_id != binding_id
        || session.binding_digest != binding_digest
        || session.harness_version.as_deref().unwrap_or_default() != harness_version
    {
        return Err(CodexSessionError::Intent);
    }
    Ok(())
}

mod head_continuation;
pub(super) use head_continuation::{next_runtime_epoch, store_error};

#[cfg(test)]
mod undo_tests;
#[cfg(test)]
mod version_tests;

#[cfg(all(test, windows))]
mod browser_mcp_tests;
