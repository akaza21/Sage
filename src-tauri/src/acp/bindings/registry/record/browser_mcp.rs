use super::{AdmittedBinding, AdmittedBindingRecord};

impl AdmittedBindingRecord<'_> {
    pub(crate) fn browser_mcp_version_notice(&self) -> Option<&'static str> {
        match &self.binding {
            AdmittedBinding::Codex(binding) => {
                let supported = binding.capability_ceiling().is_ok_and(|ceiling| {
                    crate::acp_protocol::codex_protocol::speaks_pinned_protocol(
                        ceiling.protocol_version.as_str(),
                    )
                });
                (!supported).then_some("Browser tools are unavailable for this Codex version. You can continue chatting without browser tools.")
            }
            _ => None,
        }
    }
}
