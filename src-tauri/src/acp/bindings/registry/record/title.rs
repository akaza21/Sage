use super::{Admission, AdmittedBindingRecord};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TitleCapability {
    Receive,
    ReadExisting,
    GenerateIsolated,
    Unsupported,
}

impl AdmittedBindingRecord<'_> {
    /// Curated admission metadata chooses the provider; protocol code never
    /// guesses support from a harness name or a portable ACP extension.
    pub(crate) fn title_capability(&self) -> TitleCapability {
        use TitleCapability::*;
        if !self.record.session_operations_enabled() {
            return Unsupported;
        }
        match &self.record.admission {
            Admission::Codex(_)
                if self.record.bound_build().is_some_and(|build| {
                    build.characterisation
                        == sage_domain::harness::HarnessCharacterisation::Characterised
                }) =>
            {
                GenerateIsolated
            }
            Admission::OpenCode(_)
                if self.record.bound_build().is_some_and(|build| {
                    build.characterisation
                        == sage_domain::harness::HarnessCharacterisation::Characterised
                }) =>
            {
                ReadExisting
            }
            Admission::Codex(_)
            | Admission::OpenCode(_)
            | Admission::Claude(_)
            | Admission::ClaudeSdk(_)
            | Admission::CustomAcp(_)
            | Admission::PiNative(_) => Receive,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            Admission::Pi(_) => Unsupported,
        }
    }
}
