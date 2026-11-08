//! Reads the files a prompt's `@` mentions name, before it is dispatched.
use super::*;
use crate::{
    acp::{
        contract::SageAgentSessionId,
        intent::Prompt,
        prompt_mentions::{Mentions, mention_tokens, resolve},
    },
    workspace::Workspace,
};

/// The `@` tokens of `typed`, the person's own text: never attached context,
/// which may quote untrusted pages.
pub(super) fn tokens(typed: &Prompt) -> Vec<String> {
    mention_tokens(typed.as_str())
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The files `tokens` name, resolved in the project's roots off the async
/// runtime. Embedded text is read only when the session advertises embedded
/// context, within the `budget` the dispatch leaves.
pub(super) async fn resolve_mentions(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    session_id: &SageAgentSessionId,
    tokens: Vec<String>,
    budget: usize,
) -> Result<Mentions, SendAcpTurnFailure> {
    if tokens.is_empty() {
        return Ok(Mentions::default());
    }
    let embed = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .map_err(store_error)
        .map_err(prompt_not_dispatched)?
        .sessions
        .as_slice()
        .iter()
        .any(|s| &s.id == session_id && s.capabilities.prompt_resources);
    let roots: Vec<Workspace> = state
        .database
        .root_records(project_id.as_str())
        .map_err(|_| prompt_not_dispatched(internal_error()))?
        .iter()
        // An unavailable root names no file; its mentions stay text.
        .filter_map(|root| Workspace::open(project_id.as_str(), &root.id, &root.absolute_path).ok())
        .collect();
    tauri::async_runtime::spawn_blocking(move || resolve(&tokens, &roots, embed, budget))
        .await
        .map_err(|_| prompt_not_dispatched(internal_error()))
}
