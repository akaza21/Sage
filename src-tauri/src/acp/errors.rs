use sage_acp_protocol::contract::SafeText;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionControlError {
    #[error("the harness declined the session control")]
    Refused(Option<SafeText<512>>),
    #[error("the session cannot take this control now")]
    Unavailable,
    #[error("the harness did not answer in time and may still finish")]
    Late,
    #[error("the session control outcome is unknown")]
    RecoveryRequired,
}
