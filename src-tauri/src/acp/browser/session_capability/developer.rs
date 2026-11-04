//! The developer-mode policy an attached tab's gate runs under (ADR-0009b):
//! the Settings switch and the audit log live in the settings store; the
//! grant lives on the attachment, in memory only.

use std::sync::Arc;

use crate::{
    acp::browser::{
        attachment::AttachmentLease,
        developer::{self, CdpAuditEntry, DeveloperModePolicy},
    },
    settings_store::SettingsStore,
};

pub(crate) struct LeaseDeveloperPolicy {
    lease: AttachmentLease,
    settings: SettingsStore,
    /// The switch generation when this call started; see
    /// `developer::switch_generation`.
    generation: u64,
    resource_generation: u64,
}

impl LeaseDeveloperPolicy {
    pub(crate) fn new(lease: AttachmentLease, settings: SettingsStore) -> Arc<Self> {
        let resource_generation = lease.resource_generation();
        Arc::new(Self {
            lease,
            settings,
            generation: developer::switch_generation(),
            resource_generation,
        })
    }
}

impl DeveloperModePolicy for LeaseDeveloperPolicy {
    fn enabled(&self) -> bool {
        // Only WebView2 has CDP, and an unreadable switch is off.
        cfg!(windows)
            && developer::switch_generation() == self.generation
            && self.lease.resource_generation() == self.resource_generation
            && self.settings.browser_developer_mode().unwrap_or(false)
    }

    fn granted(&self, session: &str, origin: &str) -> bool {
        self.lease.developer_granted(session, origin)
    }

    fn grant(&self, session: &str, origin: &str) -> bool {
        let settings = self.settings.clone();
        let (session_id, grant_origin, tab_id) = (session.to_owned(), origin.to_owned(), self.lease.tab_number);
        let audit = Arc::new(move |outcome| record(&settings, &CdpAuditEntry {
            session_id: session_id.clone(), tab_id, origin: grant_origin.clone(), method: "Grant".into(),
            params: String::new(), result_bytes: None, outcome,
        }));
        self.enabled()
            && self
                .lease
                .grant_developer_mode_audited_at(session, origin, self.generation, self.resource_generation, audit)
    }

    fn audit(&self, entry: CdpAuditEntry) -> bool {
        record(&self.settings, &entry)
    }
}

fn record(settings: &SettingsStore, entry: &CdpAuditEntry) -> bool {
    match settings.record_cdp_audit(entry, crate::persistence::now_millis()) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "browser developer-mode audit row was not written");
            false
        }
    }
}

#[cfg(test)]
#[path = "developer_tests.rs"]
mod tests;

/// Audits a `cdp` call refused before it reached the gate: a malformed or
/// oversized request, no attachment, Take over, a stale tab or owner.
pub(crate) fn audit_refused(
    settings: &SettingsStore,
    request: &crate::acp::browser::BrowserRequest,
    failure: crate::acp::browser::BrowserFailure,
) {
    let crate::acp::browser::BrowserCommand::Cdp { method, params } = &request.command else {
        return;
    };
    let mut method = method.clone();
    if method.len() > 128 {
        let mut end = 128;
        while !method.is_char_boundary(end) {
            end -= 1;
        }
        method.truncate(end);
    }
    if method.is_empty() {
        method.push('?');
    }
    record(
        settings,
        &CdpAuditEntry {
            session_id: request.scope.session_id.clone(),
            tab_id: request.scope.tab_id,
            origin: String::new(),
            method,
            params: serde_json::to_string(params).unwrap_or_default(),
            result_bytes: None,
            outcome: crate::acp::browser::developer::CdpAuditOutcome::Failed(failure),
        },
    );
}
