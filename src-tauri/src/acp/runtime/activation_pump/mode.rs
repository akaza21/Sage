use crate::acp::{
    contract::SageTurnId,
    intent::{ExternalId, ModelId},
};

/// How an activation opens its native session.
#[derive(Clone)]
pub(crate) enum ActivationMode {
    New {
        native_root: String,
        model: Option<ModelId>,
    },
    Resume {
        external_session_id: ExternalId,
        native_root: String,
        model: Option<ModelId>,
        /// Resume the native thread as it stood before this Turn, which the
        /// user undid but the harness still holds.
        rewind_before: Option<SageTurnId>,
    },
    #[allow(dead_code, reason = "only tests construct this variant so far")]
    Load { external_session_id: ExternalId },
}
