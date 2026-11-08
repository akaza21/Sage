//! Continues a Chat whose parked Codex session was started by an earlier
//! release: the pinned binding resumes its native thread in a new session
//! linked to it.

use crate::acp::{
    bindings::codex::{CODEX_BINDING_ID, CODEX_PINNED_VERSION},
    contract::SafeText,
    store::ActivateSessionCommand,
};

use super::{super::AcpService, CodexSessionError, DurableSession, load_durable_session};

impl AcpService {
    /// Continues a Chat whose parked head session was started by an earlier
    /// Codex release: a new head session on the pinned binding links to it,
    /// and the activation is redirected to that session's first epoch. Any
    /// other activation passes through unchanged.
    pub(in crate::acp::service) fn resume_earlier_codex_session(
        &self,
        admitted: &crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: ActivateSessionCommand,
    ) -> Result<ActivateSessionCommand, CodexSessionError> {
        let identity = admitted.identity();
        let session = load_durable_session(
            &self.database,
            &command.project_id,
            &command.chat_id,
            &command.session_id,
        )?;
        if !continues_on_pinned_codex(&session, admitted.binding_id(), identity.harness_version) {
            return Ok(command);
        }
        let linked = {
            let mut connection = self
                .database
                .write_connection()
                .map_err(|_| CodexSessionError::Intent)?;
            crate::acp::store::link_resumed_session(
                &mut connection,
                crate::acp::store::ResumeEarlierSessionCommand {
                    project_id: command.project_id.clone(),
                    chat_id: command.chat_id.clone(),
                    expected_projection_revision: command.expected_projection_revision,
                    earlier_session_id: command.session_id.clone(),
                    binding_digest: identity.binding_digest_hex.to_owned(),
                    harness_version: SafeText::new("harnessVersion", identity.harness_version)
                        .map_err(|_| CodexSessionError::Intent)?,
                    now_millis: command.now_millis,
                },
            )
            .map_err(|error| {
                crate::acp::runtime::SessionCoordinatorError::from(
                    crate::acp::runtime::JournalError::Store(error),
                )
            })?
        };
        let runtime_epoch = next_runtime_epoch(&self.database, &linked.session_id)?;
        Ok(ActivateSessionCommand {
            expected_projection_revision: linked.projection_revision,
            session_id: linked.session_id,
            runtime_epoch,
            ..command
        })
    }
}

/// The first runtime epoch a session has not used yet.
fn next_runtime_epoch(
    database: &crate::persistence::Database,
    session: &crate::acp::contract::SageAgentSessionId,
) -> Result<crate::acp::contract::RuntimeEpoch, CodexSessionError> {
    let latest: i64 = database
        .connection()
        .query_row(
            "SELECT COALESCE(MAX(runtime_epoch),0) FROM session_runtime_epochs WHERE session_id=?1",
            [session.as_str()],
            |row| row.get(0),
        )
        .map_err(|_| CodexSessionError::Intent)?;
    latest
        .checked_add(1)
        .and_then(|next| u64::try_from(next).ok())
        .and_then(|next| crate::acp::contract::RuntimeEpoch::new(next).ok())
        .ok_or(CodexSessionError::Intent)
}

/// Whether a parked session's native thread was started by an earlier Codex
/// release than the pinned one, which resumes it in a new linked session.
/// A later or unreadable version never qualifies.
pub(super) fn continues_on_pinned_codex(
    session: &DurableSession,
    binding_id: &str,
    harness_version: &str,
) -> bool {
    session.binding_id == CODEX_BINDING_ID
        && binding_id == CODEX_BINDING_ID
        && harness_version == CODEX_PINNED_VERSION
        && session
            .harness_version
            .as_deref()
            .is_some_and(|stored| precedes(stored, harness_version))
        && session.state == "ready"
        && session.residency == "cold"
        && session.external_session_id.is_some()
}

/// Whether `earlier` is a strictly lower `major.minor.patch` release than
/// `later`. Unreadable versions are never ordered.
fn precedes(earlier: &str, later: &str) -> bool {
    let parse = |version: &str| {
        let parts = version
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        (parts.len() == 3).then_some(parts)
    };
    matches!((parse(earlier), parse(later)), (Some(earlier), Some(later)) if earlier < later)
}
