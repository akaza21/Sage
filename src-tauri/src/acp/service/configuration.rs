use std::sync::Arc;

use crate::{
    acp::{
        bindings::registry::BindingRegistry,
        runtime::{NotificationSink, ProjectionNotifier},
        store::StoreError,
    },
    persistence::Database,
};

#[cfg(test)]
use crate::acp::bindings::codex::CodexBindingRegistration;

use super::AcpService;
#[cfg(all(windows, feature = "acp-fixture"))]
use super::CharacterizationBinding;

pub(super) struct ServiceConfiguration {
    pub(super) registry: Arc<BindingRegistry>,
    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(super) characterization: Option<CharacterizationBinding>,
}

impl AcpService {
    #[cfg(test)]
    pub(crate) fn new(database: Database) -> Result<Self, StoreError> {
        Self::new_with_registry_for_test(database, Arc::new(BindingRegistry::default()))
    }

    #[cfg(test)]
    pub(crate) fn new_with_notification_sink(
        database: Database,
        sink: Arc<dyn NotificationSink>,
    ) -> Result<Self, StoreError> {
        Self::new_with_notification_sink_and_registry(
            database,
            sink,
            Arc::new(BindingRegistry::default()),
        )
    }

    pub(crate) fn new_with_notification_sink_and_registry(
        database: Database,
        sink: Arc<dyn NotificationSink>,
        registry: Arc<BindingRegistry>,
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
                registry,
                #[cfg(all(windows, feature = "acp-fixture"))]
                characterization: None,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_registry_for_test(
        database: Database,
        registry: Arc<BindingRegistry>,
    ) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
            ServiceConfiguration {
                registry,
                #[cfg(all(windows, feature = "acp-fixture"))]
                characterization: None,
            },
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_codex_for_test(
        database: Database,
        codex: CodexBindingRegistration,
    ) -> Result<Self, StoreError> {
        Self::new_with_registry_for_test(database, Arc::new(BindingRegistry::with_codex(codex)))
    }

    #[cfg(test)]
    pub(crate) fn new_with_codex_publisher_for_test(
        database: Database,
        codex: CodexBindingRegistration,
        publisher: Arc<dyn crate::acp::runtime::ProjectionPublisher>,
    ) -> Result<Self, StoreError> {
        Self::new_with_registry_publisher_for_test(
            database,
            Arc::new(BindingRegistry::with_codex(codex)),
            publisher,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_registry_publisher_for_test(
        database: Database,
        registry: Arc<BindingRegistry>,
        publisher: Arc<dyn crate::acp::runtime::ProjectionPublisher>,
    ) -> Result<Self, StoreError> {
        Self::build(
            database,
            publisher,
            None,
            ServiceConfiguration {
                registry,
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
        registry: Arc<BindingRegistry>,
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
                registry,
                characterization: Some(binding),
            },
        )
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn new_characterization_for_test(
        database: Database,
        binding: CharacterizationBinding,
        registry: Arc<BindingRegistry>,
    ) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
            ServiceConfiguration {
                registry,
                characterization: Some(binding),
            },
        )
    }
}
