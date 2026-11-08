//! Catalog entries for the user-defined ACP agents in Settings. A slot is
//! listed once the user configured it: available when its executable was
//! admitted, unavailable (with a reason) when it was not.

use crate::{
    domain::{HarnessDefinition, HarnessInstallState, HarnessSignIn},
    settings_store::{MAX_CUSTOM_ACP_AGENTS, custom_acp_id, custom_acp_slot},
};

use super::{
    HarnessCatalog,
    catalog_definition::{CuratedProtocol, definition},
};

const DESCRIPTION: &str = "Your ACP agent, launched directly from the executable set in Settings.";

impl HarnessCatalog {
    pub(super) fn custom_acp_definitions(&self) -> Vec<HarnessDefinition> {
        (1..=MAX_CUSTOM_ACP_AGENTS)
            .filter_map(|slot| {
                let registration = self.registry.custom_acp(slot)?;
                let name = registration.display_name()?;
                let id = custom_acp_id(slot);
                let state = self.custom_acp_install_state(&id)?;
                Some(definition(
                    &id,
                    &name,
                    DESCRIPTION,
                    state.as_str(),
                    "custom",
                    CuratedProtocol::acp(Some(registration.profile().binding_id)),
                ))
            })
            .collect()
    }

    /// `None` while the slot holds no agent, so the catalog omits it.
    pub(super) fn custom_acp_install_state(&self, id: &str) -> Option<HarnessInstallState> {
        let registration = self.registry.custom_acp(custom_acp_slot(id)?)?;
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
        let Some(notice) = custom_acp_slot(&definition.id)
            .and_then(|slot| self.registry.custom_acp(slot))
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
        domain::HarnessTransport,
        harnesses::HarnessCatalog,
        settings_store::CustomAcpAgent,
    };

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
        let root = crate::test_paths::temp_dir()
            .join(format!("sage-custom-catalog-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("agent.exe");
        std::fs::write(&executable, b"agent").unwrap();
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
            agent("custom-acp-3", &root.join("missing.exe")),
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
                crate::settings_store::SessionAccessMode::Ask,
                crate::settings_store::SessionAccessMode::ReadOnly
            ]
        );
        assert_eq!(custom[1].install_state, "unavailable");
        assert!(custom[1].unavailable_reason.is_some());
        assert_eq!(
            catalog.harness_id_for_binding("custom-acp-1").as_deref(),
            Some("custom-acp-1")
        );
        assert_eq!(
            catalog.install_state_of("custom-acp-2"),
            None,
            "an empty slot is not in the catalog"
        );

        registry
            .custom_acp(1)
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
}
