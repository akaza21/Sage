use serde::Serialize;

use sage_acp_protocol::contract::RequiredText;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CodexAccountSnapshot {
    ManagedChatgpt,
    ApiKey,
    ProviderManaged,
    SignedOut,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexModelSnapshot {
    pub(crate) id: RequiredText<512>,
    pub(crate) name: RequiredText<512>,
    pub(crate) is_default: bool,
    /// The thinking levels the model takes, as its thread's effort option
    /// will offer them.
    pub(crate) effort_levels: Vec<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexDiscoveryContent {
    pub(crate) account: CodexAccountSnapshot,
    pub(crate) models: Vec<CodexModelSnapshot>,
    pub(crate) default_model_id: Option<RequiredText<512>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProtocolProgress {
    More,
    AwaitingAccount,
    Complete(CodexDiscoveryContent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexDiscoveryError {
    #[error("Codex discovery protocol failed")]
    Protocol,
    #[error("Codex account is outside the admitted managed ChatGPT policy")]
    AccountPolicy,
    #[error("Codex model catalog violates its bounded schema")]
    ModelCatalog,
    #[error("Codex returned an empty model catalog")]
    EmptyCatalog,
    #[error("Codex model pagination repeated a cursor")]
    CursorLoop,
    #[error("Codex model pagination exceeded four pages")]
    PageLimit,
}
