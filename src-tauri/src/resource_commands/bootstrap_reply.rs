//! The bootstrap command's reply: the frozen snapshot transport plus the
//! listing its selected harness gave when it was last asked.

use sage_domain::{
    harness::{HarnessDefinition, HarnessTransport, ModelOption},
    project::BootstrapSnapshot,
};

use crate::settings_store::SettingsStore;

/// The snapshot, with the listing its selected harness gave when last asked:
/// a new chat shows it at once while the live listing answers, and never
/// sends with it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapReply {
    #[serde(flatten)]
    snapshot: BootstrapSnapshot,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    remembered_models: Vec<ModelOption>,
}

impl BootstrapReply {
    pub(super) fn new(snapshot: BootstrapSnapshot, settings: &SettingsStore) -> Self {
        let remembered_models = snapshot
            .selected_harness_id
            .as_deref()
            .filter(|id| kept_by_projection(&snapshot.harnesses, id))
            .and_then(|id| settings.remembered_harness_models(id).ok())
            .unwrap_or_default();
        Self {
            snapshot,
            remembered_models,
        }
    }
}

/// Mirrors the WebView's choice (`preferredAvailableHarness`): a harness still
/// being checked, or an available one with a binding where it needs one.
/// Another choice there would show this harness's listing under another.
fn kept_by_projection(harnesses: &[HarnessDefinition], id: &str) -> bool {
    harnesses.iter().any(|harness| {
        harness.id == id
            && (harness.install_state == "detecting"
                || (harness.version_checking && harness.install_state != "available")
                || (harness.install_state == "available"
                    && (harness.transport != HarnessTransport::Acp
                        || harness.binding_id.is_some())))
    })
}

#[cfg(test)]
mod remembered_listing_tests {
    use super::kept_by_projection;
    use crate::harnesses::HarnessCatalog;

    #[test]
    fn a_listing_goes_only_with_a_harness_the_webview_keeps() {
        // Before discovery settles every curated harness is still detected.
        let detecting = HarnessCatalog::default().definitions();
        assert!(kept_by_projection(&detecting, "codex"));
        let mut settled = detecting.clone();
        for harness in &mut settled {
            harness.install_state = "unavailable".into();
            harness.version_checking = false;
        }
        assert!(!kept_by_projection(&settled, "codex"));
        assert!(!kept_by_projection(&detecting, "not-a-harness"));
    }
}
