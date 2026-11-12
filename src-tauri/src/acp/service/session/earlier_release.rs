//! Continues a Chat whose parked session was started by an earlier release
//! of its harness: the updated binding resumes the native session in a new
//! session linked to it.

use crate::acp::{
    bindings::{claude::sdk::CLAUDE_SDK_BINDING_ID, release_version::ReleaseVersion},
    contract::SafeText,
    store::ActivateSessionCommand,
};

use super::{super::AcpService, CodexSessionError, DurableSession, load_durable_session};

impl AcpService {
    /// Continues a Chat whose parked head session was started by an earlier
    /// release of its harness: a new head session on the updated binding
    /// links to it, and the activation is redirected to that session's first
    /// epoch, also when a retry names the earlier session after a refused
    /// activation. Any other activation passes through unchanged.
    pub(in crate::acp::service) fn resume_earlier_release_session(
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
        let linked = if session.state == "closed" {
            // A retry after a refused activation still names the earlier
            // session; it continues in the linked head that has not started.
            let head = crate::acp::store::unstarted_resume_link(
                &self.database.connection(),
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
            );
            match head.map_err(store_error)? {
                Some(linked) => linked,
                None => return Ok(command),
            }
        } else if continues_on_later_release(
            &session,
            admitted.binding_id(),
            identity.harness_version,
        ) && resumes_sessions(admitted)?
        {
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
            .map_err(store_error)?
        } else {
            return Ok(command);
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

/// A store failure surfaces as the session coordination error it would be
/// on the journal path.
fn store_error(error: crate::acp::store::StoreError) -> CodexSessionError {
    crate::acp::runtime::SessionCoordinatorError::from(crate::acp::runtime::JournalError::Store(
        error,
    ))
    .into()
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

/// Whether the admitted build can resume a native session. Linking a
/// session it cannot resume would only defer the refusal to activation.
fn resumes_sessions(
    admitted: &crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
) -> Result<bool, CodexSessionError> {
    admitted
        .supported_capabilities()
        .map(|capabilities| capabilities.resume_session)
        .map_err(|error| CodexSessionError::Binding(error.into()))
}

/// Whether a parked session's native session was started by an earlier
/// release of the same binding than the admitted one, which resumes it in a
/// new linked session. A later, equal or unreadable version never
/// qualifies, so a downgrade or a different build of the same release is
/// still refused. Claude Code SDK sessions follow the CLI in place instead.
pub(super) fn continues_on_later_release(
    session: &DurableSession,
    binding_id: &str,
    harness_version: &str,
) -> bool {
    session.binding_id == binding_id
        && binding_id != CLAUDE_SDK_BINDING_ID
        && session
            .harness_version
            .as_deref()
            .is_some_and(|stored| precedes(stored, harness_version))
        && session.state == "ready"
        && session.residency == "cold"
        && session.external_session_id.is_some()
}

/// Whether `earlier` is a strictly lower release than `later`. Unreadable
/// versions and prereleases on either side are never ordered, so a chat only
/// moves between final releases.
fn precedes(earlier: &str, later: &str) -> bool {
    matches!(
        (ReleaseVersion::parse(earlier), ReleaseVersion::parse(later)),
        (Some(earlier), Some(later))
            if !earlier.is_prerelease() && !later.is_prerelease() && earlier < later
    )
}
