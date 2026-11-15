//! Reads the files a prompt's `@` mentions name, before it is dispatched.
//!
//! Steer carries no mentions: every binding that accepts it sends its text
//! alone, so a resolved file would never reach the harness.
use super::*;
use crate::{
    acp::{
        prompt_mentions::{MentionRoot, Mentions, mention_tokens, resolve},
        transcript_quote::Prompt,
    },
    workspace::Workspace,
};
use sage_acp_protocol::{contract::SageAgentSessionId, intent::BindingId};

/// The `@` tokens of `typed`, the person's own text: never attached context,
/// which may quote untrusted pages.
pub(super) fn tokens(typed: &Prompt) -> Vec<String> {
    mention_tokens(typed.as_str())
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The files `tokens` name, resolved in the project's roots off the async
/// runtime, for a binding whose prompts carry content blocks. Embedded text
/// is read only when the session advertises embedded context, and every
/// mention is charged to the `budget` the dispatch leaves.
pub(super) async fn resolve_mentions(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    (binding_id, session_id): (&BindingId, &SageAgentSessionId),
    tokens: Vec<String>,
    budget: usize,
) -> Result<Mentions, SendAcpTurnFailure> {
    let sends_blocks = state
        .acp
        .binding_registry()
        .route(binding_id.as_str())
        .is_ok_and(|record| record.sends_prompt_blocks());
    if tokens.is_empty() || !sends_blocks {
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
    let records = state
        .database
        .root_records(project_id.as_str())
        .map_err(|_| prompt_not_dispatched(internal_error()))?;
    let project = project_id.as_str().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let roots: Vec<MentionRoot> = records
            .iter()
            // An unavailable root names no file; its mentions stay text.
            .filter_map(|root| {
                Some(MentionRoot {
                    workspace: Workspace::open(&project, &root.id, &root.absolute_path).ok()?,
                    name: crate::project_registry::folder_name(&root.absolute_path),
                })
            })
            .collect();
        resolve(&tokens, &roots, embed, budget)
    })
    .await
    .map_err(|_| prompt_not_dispatched(internal_error()))
}
