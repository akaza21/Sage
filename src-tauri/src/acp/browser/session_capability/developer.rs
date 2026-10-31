//! The developer-mode policy an attached tab's gate runs under (ADR-0009b):
//! the Settings switch and the audit log live in the settings store; the
//! grant lives on the attachment, in memory only.

use std::sync::Arc;

use crate::{
    acp::browser::{
        attachment::AttachmentLease,
        developer::{CdpAuditEntry, DeveloperModePolicy},
    },
    settings_store::SettingsStore,
};

pub(crate) struct LeaseDeveloperPolicy {
    lease: AttachmentLease,
    settings: SettingsStore,
}

impl LeaseDeveloperPolicy {
    pub(crate) fn new(lease: AttachmentLease, settings: SettingsStore) -> Arc<Self> {
        Arc::new(Self { lease, settings })
    }
}

impl DeveloperModePolicy for LeaseDeveloperPolicy {
    fn enabled(&self) -> bool {
        // An unreadable switch is off.
        self.settings.browser_developer_mode().unwrap_or(false)
    }

    fn granted(&self, session: &str, origin: &str) -> bool {
        self.lease.developer_granted(session, origin)
    }

    fn grant(&self, session: &str, origin: &str) -> bool {
        self.lease.grant_developer_mode(session, origin)
    }

    fn audit(&self, entry: CdpAuditEntry) {
        if let Err(error) = self
            .settings
            .record_cdp_audit(&entry, crate::persistence::now_millis())
        {
            tracing::warn!(%error, "browser developer-mode audit row was not written");
        }
    }
}
