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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeveloperGrant {
    session: String,
    origin: String,
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

    /// Records the grant unless the Settings switch changed since
    /// `generation` was read; checked under the grant's own lock, which the
    /// switch going off takes to end grants.
    pub(crate) fn grant_developer_mode_at(
        &self,
        session: &str,
        origin: &str,
        generation: u64,
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
        let Ok(mut grant) = self.developer.lock() else {
            return false;
        };
        if developer::switch_generation() != generation {
            return false;
        }
        *grant = Some(DeveloperGrant {
            session: session.to_owned(),
            origin: origin.to_owned(),
        });
        true
    }

    pub(crate) fn developer_granted(&self, session: &str, origin: &str) -> bool {
        self.is_live()
            && !self.user_controls()
            && self.developer.lock().is_ok_and(|grant| {
                grant
                    .as_ref()
                    .is_some_and(|grant| grant.session == session && grant.origin == origin)
            })
    }

    /// Whether the chrome shows the Developer mode tag for this attachment.
    pub(crate) fn developer_mode_active(&self) -> bool {
        self.is_live()
            && !self.user_controls()
            && self.developer.lock().is_ok_and(|grant| grant.is_some())
    }

    pub(super) fn end_developer_mode(&self) -> bool {
        self.developer
            .lock()
            .map(|mut grant| grant.take().is_some())
            .unwrap_or(false)
    }

    /// Keeps the grant across a same-origin commit (a reload, a link within
    /// the site) and ends it on any other.
    pub(super) fn end_developer_mode_unless_on(&self, url: &str) {
        if let Ok(mut grant) = self.developer.lock()
            && grant
                .as_ref()
                .is_some_and(|grant| origin_of(url).as_deref() != Some(grant.origin.as_str()))
        {
            *grant = None;
        }
    }
}

impl BrowserAttachments {
    /// Ends every developer-mode grant, as the Settings switch going off
    /// does. Returns the tabs that held one.
    pub(crate) fn end_developer_mode(&self) -> Vec<String> {
        let mut labels = Vec::new();
        let mut end = |lease: &AttachmentLease| {
            if lease.end_developer_mode() {
                labels.push(lease.tab_label.clone());
            }
        };
        if let Ok(entries) = self.entries.lock() {
            entries.values().for_each(&mut end);
        }
        if let Ok(entries) = self.agent_entries.lock() {
            entries.values().for_each(&mut end);
        }
        labels
    }
}

#[cfg(test)]
#[path = "developer_tests.rs"]
mod tests;
