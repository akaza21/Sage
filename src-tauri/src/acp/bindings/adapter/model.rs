use super::*;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterIdentity {
    pub(crate) binding_digest: [u8; 32],
    pub(crate) runtime_epoch: RuntimeEpoch,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) external_id: ExternalId,
    pub(crate) turn_id: Option<ExternalId>,
    pub(crate) turn_client_message_id: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct AdapterModel {
    pub(crate) id: ModelId,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
}

impl AdapterModel {
    pub(crate) fn actual_bytes(&self) -> Option<usize> {
        self.id.as_str().len().checked_add(self.name.as_str().len())
    }
}
