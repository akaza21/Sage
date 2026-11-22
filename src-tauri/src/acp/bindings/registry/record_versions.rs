//! Which release a harness's binding runs, and the release a preferred binding
//! needs, for the version rows in Settings.

use super::{Admission, BindingRecord, BindingRegistry};

impl BindingRecord {
    /// The harness release this binding runs when it is not the installed CLI
    /// (an adapter that bundles its own); `None` when it runs the installed one.
    pub(crate) fn runs_version(&self) -> Option<String> {
        match &self.admission {
            Admission::Claude(admission) => admission.admitted()?.runs_version().map(Into::into),
            Admission::Codex(_)
            | Admission::ClaudeSdk(_)
            | Admission::OpenCode(_)
            | Admission::CustomAcp(_)
            | Admission::PiNative(_) => None,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(_) => None,
        }
    }

    /// The oldest release this binding admits, while it refuses the installed
    /// one as older than that.
    fn refused_below(&self) -> Option<&'static str> {
        match &self.admission {
            Admission::ClaudeSdk(admission) => admission.refused_below(),
            // These admit or refuse the installed release without a fallback
            // binding for the same harness to run meanwhile.
            Admission::Codex(_)
            | Admission::Claude(_)
            | Admission::OpenCode(_)
            | Admission::CustomAcp(_)
            | Admission::PiNative(_) => None,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(_) => None,
        }
    }
}

impl BindingRegistry {
    /// When the harness runs on a fallback binding because another binding
    /// refused the installed release as too old, the release that binding needs.
    pub(crate) fn direct_binding_minimum(&self, harness_id: &str) -> Option<&'static str> {
        let serving = self.for_harness(harness_id)?;
        [
            &self.codex,
            &self.claude,
            &self.claude_sdk,
            &self.opencode,
            &self.pi_native,
        ]
        .into_iter()
        .filter(|record| record.serves_harness(harness_id) && !std::ptr::eq(*record, serving))
        .find_map(BindingRecord::refused_below)
    }
}
