use crate::acp::intent::{ExternalId, ModelId};

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
        /// Resume at this durably resolved native reply after an undo.
        rewind_at: Option<String>,
    },
    #[allow(dead_code, reason = "only tests construct this variant so far")]
    Load { external_session_id: ExternalId },
}
