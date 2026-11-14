use super::{CuratedProtocol, HarnessCatalog, HarnessDefinition, definition};
use sage_domain::harness::HarnessInstallState;

impl HarnessCatalog {
    /// Restore display identity without granting launch authority or changing
    /// the durable binding. These entries never enter the curated registry.
    pub(crate) fn restore_binding(
        &self,
        binding_id: &str,
        definitions: &mut Vec<HarnessDefinition>,
    ) -> String {
        if let Some(id) = self.harness_id_for_binding(binding_id) {
            return id;
        }
        if let Some(existing) = definitions
            .iter()
            .find(|entry| entry.binding_id.as_deref() == Some(binding_id))
        {
            return existing.id.clone();
        }
        let id = format!("unavailable-binding:{binding_id}");
        definitions.push(definition(
            &id,
            "Unavailable harness",
            "The harness bound to this Chat is not available in this build.",
            HarnessInstallState::Unavailable.as_str(),
            "custom",
            CuratedProtocol::acp(Some(binding_id)),
        ));
        id
    }
}
