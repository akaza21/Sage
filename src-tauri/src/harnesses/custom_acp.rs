//! Catalog entries for the user-defined ACP agents in Settings. An agent is
//! listed while it exists: available when its executable was admitted,
//! unavailable (with a reason) when it was not. A removed agent's id is
//! never listed again, so its chats show the harness as unavailable.

use sage_domain::harness::{HarnessDefinition, HarnessInstallState, HarnessSignIn};

use super::{
    HarnessCatalog,
    catalog_definition::{CuratedProtocol, definition},
};

const DESCRIPTION: &str = "Your ACP agent, launched directly from the executable set in Settings.";

impl HarnessCatalog {
    pub(super) fn custom_acp_definitions(&self) -> Vec<HarnessDefinition> {
        let mut entries: Vec<_> = self
            .registry
            .custom_acp_registrations()
            .filter_map(|registration| {
                let id = registration.agent_id()?;
                let name = registration.display_name()?;
                let state = self.custom_acp_install_state(&id)?;
                Some(definition(
                    &id,
                    &name,
                    DESCRIPTION,
                    state.as_str(),
                    "custom",
                    CuratedProtocol::acp(Some(&id)),
                ))
            })
            .collect();
        // Oldest first, as Settings lists them.
        entries.sort_by_key(|entry| crate::settings_store::custom_acp_number(&entry.id));
        entries
    }

    /// `None` when no agent has this id, so the catalog omits it.
    pub(super) fn custom_acp_install_state(&self, id: &str) -> Option<HarnessInstallState> {
        let registration = self.registry.custom_acp(id)?;
        registration.display_name()?;
        Some(if registration.admitted().is_some() {
            HarnessInstallState::Available
        } else {
            HarnessInstallState::Unavailable
        })
    }

    /// After the agent refused a session for sign-in, the catalog carries
    /// the methods it offered.
    pub(super) fn annotate_custom_sign_in(&self, definition: &mut HarnessDefinition) {
        let Some(notice) = self
            .registry
            .custom_acp(&definition.id)
            .and_then(|registration| registration.sign_in_notice())
        else {
            return;
        };
        definition.sign_in = Some(HarnessSignIn {
            command: String::new(),
            hint: notice,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        acp::bindings::{registry::BindingRegistry, standard_acp::StandardAcpBindingSource},
        harnesses::HarnessCatalog,
        settings_store::CustomAcpAgent,
    };
    use sage_domain::harness::HarnessTransport;

    fn agent(id: &str, executable: &std::path::Path) -> CustomAcpAgent {
        CustomAcpAgent {
            id: id.into(),
            name: "Team agent".into(),
            executable: executable.to_string_lossy().into_owned(),
            args: Vec::new(),
            env: Vec::new(),
        }
    }

    #[test]
    fn configured_agents_are_listed_with_their_binding_and_reason() {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-custom-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("agent.exe");
        crate::acp::bindings::standard_acp::custom_executable::write_test_executable(
            &executable,
            b"agent",
        );
        let registry = Arc::new(BindingRegistry::default());
        let catalog = HarnessCatalog::with_registry(Arc::clone(&registry)).settled_for_test();
        assert!(
            catalog
                .definitions()
                .iter()
                .all(|definition| definition.source != "custom")
        );
        registry.apply_custom_acp_agents(&[
            agent("custom-acp-1", &executable),
            agent("custom-acp-12", &root.join("missing.exe")),
        ]);
        let definitions = catalog.definitions();
        let custom: Vec<_> = definitions
            .iter()
            .filter(|definition| definition.source == "custom")
            .collect();
        assert_eq!(custom.len(), 2);
        assert_eq!(custom[0].id, "custom-acp-1");
        assert_eq!(custom[0].name, "Team agent");
        assert_eq!(custom[0].install_state, "available");
        assert_eq!(custom[0].transport, HarnessTransport::Acp);
        assert_eq!(custom[0].binding_id.as_deref(), Some("custom-acp-1"));
        assert_eq!(
            custom[0]
                .access_modes
                .iter()
                .map(|mode| mode.id)
                .collect::<Vec<_>>(),
            [
                sage_acp_protocol::access_mode::SessionAccessMode::Ask,
                sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly
            ]
        );
        assert_eq!(custom[1].install_state, "unavailable");
        assert!(custom[1].unavailable_reason.is_some());
        assert_eq!(
            catalog.harness_id_for_binding("custom-acp-1").as_deref(),
            Some("custom-acp-1")
        );
        assert_eq!(custom[1].id, "custom-acp-12");
        assert_eq!(
            catalog.install_state_of("custom-acp-2"),
            None,
            "an id no agent holds is not in the catalog"
        );

        registry
            .custom_acp("custom-acp-1")
            .unwrap()
            .record_sign_in("Team agent needs sign-in. It offers: Browser.");
        let signed = catalog.definitions();
        let entry = signed.iter().find(|d| d.id == "custom-acp-1").unwrap();
        let sign_in = entry.sign_in.as_ref().expect("sign-in notice");
        assert!(sign_in.command.is_empty());
        assert_eq!(
            sign_in.hint,
            "Team agent needs sign-in. It offers: Browser."
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_removed_agents_id_stays_unavailable_after_a_new_agent_takes_its_registration() {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-custom-reuse-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("agent.exe");
        crate::acp::bindings::standard_acp::custom_executable::write_test_executable(
            &executable,
            b"agent",
        );
        let registry = Arc::new(BindingRegistry::default());
        let catalog = HarnessCatalog::with_registry(Arc::clone(&registry)).settled_for_test();
        let old: Vec<_> = (1..=4)
            .map(|n| agent(&format!("custom-acp-{n}"), &executable))
            .collect();
        registry.apply_custom_acp_agents(&old);
        assert!(registry.resolve("custom-acp-2").is_ok());
        // custom-acp-2 is removed and a new agent (same executable) added.
        let mut now = old.clone();
        now[1] = agent("custom-acp-5", &executable);
        registry.apply_custom_acp_agents(&now);
        assert!(registry.for_harness("custom-acp-2").is_none());
        assert!(registry.resolve("custom-acp-2").is_err());
        assert_eq!(catalog.install_state_of("custom-acp-2"), None);
        assert!(
            catalog.definitions().iter().all(|d| d.id != "custom-acp-2"),
            "a chat bound to the removed agent finds no harness"
        );
        let fresh = registry.resolve("custom-acp-5").expect("new agent");
        assert_eq!(fresh.binding_id(), "custom-acp-5");
        assert_eq!(
            catalog.install_state_of("custom-acp-5"),
            Some(sage_domain::harness::HarnessInstallState::Available)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
