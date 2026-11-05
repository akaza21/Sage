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
mod command;
mod session;
pub(crate) use command::DeveloperCommand;

pub(crate) type GrantAudit = Arc<dyn Fn(developer::CdpAuditOutcome) -> bool + Send + Sync>;

pub(crate) type AgentControlEmitter = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub(super) struct DeveloperRegistry {
    emit: Mutex<Option<AgentControlEmitter>>,
    tabs: Mutex<HashMap<String, std::sync::Weak<Mutex<TabGrants>>>>,
}

#[derive(Default)]
struct TabGrants {
    grants: HashMap<usize, DeveloperGrant>,
    release: Option<resources::DeveloperRelease>,
}

pub(super) struct DeveloperAccess {
    tab: Arc<Mutex<TabGrants>>,
    key: Arc<()>,
    label: String,
    events: Arc<DeveloperRegistry>,
}

impl DeveloperAccess {
    pub(super) fn new(events: Arc<DeveloperRegistry>, label: &str) -> Self {
        let tab = {
            let mut tabs = events
                .tabs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Weak entries never retain tabs or native state. Prune on creation
            // so this index stays bounded by currently attached tabs.
            tabs.retain(|_, tab| tab.strong_count() != 0);
            match tabs.get(label).and_then(std::sync::Weak::upgrade) {
                Some(tab) => tab,
                None => {
                    let tab = Arc::new(Mutex::new(TabGrants::default()));
                    tabs.insert(label.to_owned(), Arc::downgrade(&tab));
                    tab
                }
            }
        };
        Self {
            tab,
            key: Arc::new(()),
            label: label.to_owned(),
            events,
        }
    }

    fn key(&self) -> usize {
        Arc::as_ptr(&self.key) as usize
    }

    /// The only writer of the grant. Notify under its lock so a command
    /// cannot observe new privilege before the chrome has been notified.
    /// Return the old grant for audit I/O after releasing the state locks.
    fn transition(
        &self,
        tab: &str,
        tab_grants: &mut TabGrants,
        next: Option<DeveloperGrant>,
    ) -> Option<DeveloperGrant> {
        let previous = tab_grants.grants.remove(&self.key());
        let changed = previous.is_some() || next.is_some();
        if let Some(next) = next {
            tab_grants.grants.insert(self.key(), next);
        }
        if changed && let Some(emit) = self.events.emit.lock().ok().and_then(|slot| slot.clone()) {
            emit(tab);
        }
        tab_grants.release_ungranted();
        previous
    }
}

struct DeveloperGrant {
    identity: Arc<()>,
    session: String,
    origin: String,
    audit: GrantAudit,
    active: std::sync::Weak<AtomicBool>,
    retiring: std::sync::Weak<AtomicBool>,
    user_control: std::sync::Weak<AtomicBool>,
    commands: usize,
    draining: bool,
    revoked: bool,
}

impl DeveloperGrant {
    fn is_live(&self) -> bool {
        !self.revoked
            && self
                .active
                .upgrade()
                .is_some_and(|active| active.load(Ordering::Acquire))
            && self
                .retiring
                .upgrade()
                .is_some_and(|retiring| !retiring.load(Ordering::Acquire))
            && self
                .user_control
                .upgrade()
                .is_some_and(|paused| !paused.load(Ordering::Acquire))
    }
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
        let Ok(mut grant) = self.developer.tab.lock() else {
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
                identity: Arc::new(()),
                session: session.to_owned(),
                origin: origin.to_owned(),
                audit,
                active: Arc::downgrade(&self.active),
                retiring: Arc::downgrade(&self.retiring),
                user_control: Arc::downgrade(&self.user_control),
                commands: 0,
                draining: false,
                revoked: false,
            }),
        );
        drop(grant);
        drop(slot);
        drop(previous);
        true
    }

    #[cfg(test)]
    pub(crate) fn developer_granted(&self, session: &str, origin: &str) -> bool {
        self.is_live()
            && !self.user_controls()
            && self.developer.tab.lock().is_ok_and(|grant| {
                grant
                    .grants
                    .get(&self.developer.key())
                    .is_some_and(|grant| grant.session == session && grant.origin == origin)
            })
    }

    /// Whether this live attachment holds developer approval. The tab's
    /// indicator aggregates this across all attachments in `agent_control`.
    pub(crate) fn developer_mode_active(&self) -> bool {
        self.is_live()
            && !self.user_controls()
            && self
                .developer
                .tab
                .lock()
                .is_ok_and(|grant| grant.grants.contains_key(&self.developer.key()))
    }

    pub(super) fn end_developer_mode(&self) -> bool {
        let ended = self.developer.tab.lock().ok().and_then(|mut grant| {
            self.developer
                .transition(&self.tab_label, &mut grant, None)
                .map(|grant| (grant, self.invalidate_grant()))
        });
        let had_grant = ended.is_some();
        if ended.is_some() {
            self.release_developer_resources();
        }
        drop(ended);
        had_grant
    }

    /// Keeps the grant across an in-site commit and ends it on any other.
    pub(super) fn end_developer_mode_unless_on(&self, url: &str) {
        let incoming = origin_of(url);
        let changed = origin_of(&self.url) != incoming;
        let ended = self.developer.tab.lock().ok().and_then(|mut grant| {
            if changed
                || grant
                    .grants
                    .get(&self.developer.key())
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
        if ended.is_some() {
            self.release_developer_resources();
        }
        drop(ended);
    }
}

impl BrowserAttachments {
    #[cfg(any(windows, test))]
    pub(crate) fn set_agent_control_emitter(&self, emit: AgentControlEmitter) {
        if let Ok(mut slot) = self.developer_registry.emit.lock() {
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
