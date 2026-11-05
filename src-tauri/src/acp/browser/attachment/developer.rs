//! The developer-mode grant an attachment holds (ADR-0009b). It lives only
//! here, in memory: Stop, detach, tab close, Take over, a cross-origin
//! commit, another session taking the tab and the Settings switch going off
//! each end it, and Hand back never restores it.
//!
//! Lock order: the authority slot, then the grant. A grant is recorded under
//! the slot lock, so Take over, Stop and a settled navigation, which change
//! the slot under the same lock, either end it or make the grant refuse.

use super::*;
use crate::acp::browser::developer;

pub(crate) type GrantAudit = Arc<dyn Fn(developer::CdpAuditOutcome) -> bool + Send + Sync>;

pub(crate) type AgentControlEmitter = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub(super) struct ControlEvents(Mutex<Option<AgentControlEmitter>>);

pub(super) struct DeveloperAccess {
    grant: Mutex<Option<DeveloperGrant>>,
    events: Arc<ControlEvents>,
}

impl DeveloperAccess {
    pub(super) fn new(events: Arc<ControlEvents>) -> Self {
        Self {
            grant: Mutex::new(None),
            events,
        }
    }

    /// The only writer of the grant. Notify under its lock so a command
    /// cannot observe new privilege before the chrome has been notified.
    /// Return the old grant for audit I/O after releasing the state locks.
    fn transition(
        &self,
        tab: &str,
        grant: &mut Option<DeveloperGrant>,
        next: Option<DeveloperGrant>,
    ) -> Option<DeveloperGrant> {
        let changed = grant.is_some() || next.is_some();
        let previous = std::mem::replace(grant, next);
        if changed && let Some(emit) = self.events.0.lock().ok().and_then(|slot| slot.clone()) {
            emit(tab);
        }
        previous
    }
}

struct DeveloperGrant {
    session: String,
    origin: String,
    audit: GrantAudit,
}

impl Drop for DeveloperGrant {
    fn drop(&mut self) {
        (self.audit)(developer::CdpAuditOutcome::Revoked);
    }
}

fn origin_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .map(|url| url.origin().ascii_serialization())
}

impl AttachmentLease {
    /// Records the user's approval for `session` on `origin`; false when the
    /// attachment, its authority or its page moved on since the card.
    #[cfg(test)]
    pub(crate) fn grant_developer_mode(&self, session: &str, origin: &str) -> bool {
        self.grant_developer_mode_at(session, origin, developer::switch_generation())
    }

    #[cfg(test)]
    pub(crate) fn grant_developer_mode_at(
        &self,
        session: &str,
        origin: &str,
        generation: u64,
    ) -> bool {
        self.grant_developer_mode_audited_at(
            session,
            origin,
            generation,
            self.resource_generation(),
            self.grant_generation(),
            Arc::new(|_| true),
        )
    }

    /// Records the grant unless the Settings switch changed since
    /// `generation` was read; checked under the grant's own lock, which the
    /// switch going off takes to end grants.
    pub(crate) fn grant_developer_mode_audited_at(
        &self,
        session: &str,
        origin: &str,
        generation: u64,
        resource_generation: u64,
        grant_generation: u64,
        audit: GrantAudit,
    ) -> bool {
        let Ok(slot) = self.authority.lock() else {
            return false;
        };
        if !self.is_live()
            || self.user_controls()
            || slot.as_ref().is_none_or(|(held, _)| held != session)
            || origin_of(&self.url).as_deref() != Some(origin)
        {
            return false;
        }
        let Ok(mut grant) = self.developer.grant.lock() else {
            return false;
        };
        if !developer::switch_is_current(generation)
            || self.resource_generation() != resource_generation
            || self.grant_generation() != grant_generation
        {
            return false;
        }
        if !audit(developer::CdpAuditOutcome::Granted) {
            return false;
        }
        let previous = self.developer.transition(
            &self.tab_label,
            &mut grant,
            Some(DeveloperGrant {
                session: session.to_owned(),
                origin: origin.to_owned(),
                audit,
            }),
        );
        drop(grant);
        drop(slot);
        drop(previous);
        true
    }

    pub(crate) fn developer_granted(&self, session: &str, origin: &str) -> bool {
        self.is_live()
            && !self.user_controls()
            && self.developer.grant.lock().is_ok_and(|grant| {
                grant
                    .as_ref()
                    .is_some_and(|grant| grant.session == session && grant.origin == origin)
            })
    }

    /// Whether the chrome shows the Developer mode tag for this attachment.
    pub(crate) fn developer_mode_active(&self) -> bool {
        self.is_live()
            && !self.user_controls()
            && self
                .developer
                .grant
                .lock()
                .is_ok_and(|grant| grant.is_some())
    }

    pub(super) fn end_developer_mode(&self) -> bool {
        let ended = self.developer.grant.lock().ok().and_then(|mut grant| {
            self.developer
                .transition(&self.tab_label, &mut grant, None)
                .map(|grant| (grant, self.invalidate_grant()))
        });
        let had_grant = ended.is_some();
        if let Some((_, generation)) = &ended {
            self.release_developer_resources(None, Some(*generation));
        }
        drop(ended);
        had_grant
    }

    /// Keeps the grant across an in-site commit and ends it on any other.
    pub(super) fn end_developer_mode_unless_on(&self, url: &str) {
        let incoming = origin_of(url);
        let changed = origin_of(&self.url) != incoming;
        let ended = self.developer.grant.lock().ok().and_then(|mut grant| {
            if changed
                || grant
                    .as_ref()
                    .is_some_and(|grant| incoming.as_deref() != Some(grant.origin.as_str()))
            {
                Some((
                    self.developer.transition(&self.tab_label, &mut grant, None),
                    self.invalidate_grant(),
                ))
            } else {
                None
            }
        });
        if let Some((_, generation)) = &ended {
            self.release_developer_resources(None, Some(*generation));
        }
        drop(ended);
    }
}

impl BrowserAttachments {
    #[cfg(any(windows, test))]
    pub(crate) fn set_agent_control_emitter(&self, emit: AgentControlEmitter) {
        if let Ok(mut slot) = self.control_events.0.lock() {
            *slot = Some(emit);
        }
    }

    /// Ends every developer-mode grant, as the Settings switch going off
    /// does. Returns the tabs that held one.
    pub(crate) fn end_developer_mode(&self) -> Vec<String> {
        let mut leases = Vec::new();
        if let Ok(entries) = self.entries.lock() {
            leases.extend(entries.values().cloned());
        }
        if let Ok(entries) = self.agent_entries.lock() {
            leases.extend(entries.values().cloned());
        }
        let mut labels = Vec::new();
        for lease in leases {
            if lease.end_developer_mode() {
                labels.push(lease.tab_label.clone());
            }
            // A developer command that passed its check before the grant
            // ended may be issuing under the live lock; wait for it, holding
            // no other lock, so none is sent after switch-off returns.
            let authority = lease
                .authority
                .lock()
                .ok()
                .and_then(|slot| slot.as_ref().map(|(_, authority)| authority.clone()));
            if let Some(authority) = authority {
                authority.fence();
            }
        }
        labels
    }
}

#[cfg(test)]
#[path = "developer_tests.rs"]
mod tests;
