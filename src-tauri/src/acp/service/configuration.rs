use std::sync::Arc;

use crate::{
    acp::{
        bindings::codex::{CodexBindingSnapshot, startup_codex_binding_snapshot},
        runtime::{NotificationSink, ProjectionNotifier},
        store::StoreError,
    },
    persistence::Database,
};

use super::AcpService;
#[cfg(all(windows, feature = "acp-fixture"))]
use super::CharacterizationBinding;

pub(super) struct ServiceConfiguration {
    pub(super) codex: CodexBindingSnapshot,
    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(super) characterization: Option<CharacterizationBinding>,
}

impl Default for ServiceConfiguration {
    fn default() -> Self {
        Self {
            codex: startup_codex_binding_snapshot(),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization: None,
        }
    }
}

impl AcpService {
    #[cfg(test)]
    pub(crate) fn new(database: Database) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
            ServiceConfiguration::default(),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_notification_sink(
        database: Database,
        sink: Arc<dyn NotificationSink>,
    ) -> Result<Self, StoreError> {
        Self::new_with_notification_sink_and_codex(database, sink, startup_codex_binding_snapshot())
    }

    pub(crate) fn new_with_notification_sink_and_codex(
        database: Database,
        sink: Arc<dyn NotificationSink>,
        codex: CodexBindingSnapshot,
    ) -> Result<Self, StoreError> {
        let runtime = tauri::async_runtime::handle();
        let notifier = Arc::new(ProjectionNotifier::start_with(sink, move |worker| {
            runtime.inner().spawn(worker)
        }));
        Self::build(
            database,
            notifier.clone(),
            Some(notifier),
            ServiceConfiguration {
                codex,
                #[cfg(all(windows, feature = "acp-fixture"))]
                characterization: None,
            },
        )
    }

    #[cfg(all(test, windows))]
    pub(crate) fn new_with_codex_for_test(
        database: Database,
        codex: CodexBindingSnapshot,
    ) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
            ServiceConfiguration {
                codex,
                #[cfg(all(windows, feature = "acp-fixture"))]
                characterization: None,
            },
        )
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn new_with_characterization_sink(
        database: Database,
        sink: Arc<dyn NotificationSink>,
        binding: CharacterizationBinding,
        codex: CodexBindingSnapshot,
    ) -> Result<Self, StoreError> {
        let runtime = tauri::async_runtime::handle();
        let notifier = Arc::new(ProjectionNotifier::start_with(sink, move |worker| {
            runtime.inner().spawn(worker)
        }));
        Self::build(
            database,
            notifier.clone(),
            Some(notifier),
            ServiceConfiguration {
                codex,
                characterization: Some(binding),
            },
        )
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn new_characterization_for_test(
        database: Database,
        binding: CharacterizationBinding,
        codex: CodexBindingSnapshot,
    ) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
            ServiceConfiguration {
                codex,
                characterization: Some(binding),
            },
        )
    }
}
