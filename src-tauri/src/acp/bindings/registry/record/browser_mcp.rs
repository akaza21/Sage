//! Whether the admitted binding can serve Sage's browser tools.

use super::{AdmittedBinding, AdmittedBindingRecord};

impl AdmittedBindingRecord<'_> {
    /// A notice when this binding's Codex build cannot speak the browser
    /// tools' pinned protocol, or `None` when the tools are available.
    pub(crate) fn browser_mcp_version_notice(&self) -> Option<&'static str> {
        match &self.binding {
            AdmittedBinding::Codex(binding) => {
                let supported = binding.capability_ceiling().is_ok_and(|ceiling| {
                    crate::acp::bindings::codex::speaks_pinned_protocol(
                        ceiling.protocol_version.as_str(),
                    )
                });
                (!supported).then_some("Browser tools are unavailable for this Codex version. You can continue chatting without browser tools.")
            }
            _ => None,
        }
    }
}
