use super::*;

impl AcpService {
    /// Adds the current session's live activities and Undo to the page.
    /// They are display decoration: a page they would push over its byte
    /// budget goes without them.
    pub(super) fn compose_session_surface(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        let Some(session) = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref())
        else {
            return Ok(());
        };
        let mut view = crate::acp::store::session_surface::view(
            &self.database.connection(),
            page.project_id.as_str(),
            page.chat_id.as_str(),
            session.id.as_str(),
            session.runtime_epoch.map(|epoch| epoch.get()),
        )?
        .unwrap_or_default();
        let latest = page
            .turns
            .as_slice()
            .iter()
            .max_by_key(|turn| (turn.created_at, turn.ordinal))
            .filter(|turn| turn.session_id == session.id && turn.terminal_at.is_some());
        let undo = latest.and_then(|turn| {
            undo_keeps_files(
                session.binding_id.as_str(),
                session.runtime_epoch.map(|epoch| epoch.get()),
                turn.runtime_epoch.get(),
            )
            .map(|keeps_files| (turn.id.clone(), keeps_files))
        });
        view.undo_keeps_files = undo.as_ref().is_some_and(|(_, keeps)| *keeps);
        view.undo_turn = undo.map(|(turn, _)| turn);
        page.session_surface = (view != Default::default()).then_some(view);
        let bytes = serde_json::to_vec(page)
            .map_err(|_| StoreError::Corrupt("chat page serialization"))?
            .len();
        if bytes > crate::acp::limits::LIMITS.durable.history_page_bytes as usize {
            page.session_surface = None;
        }
        Ok(())
    }
}

/// How a running session undoes its latest finished turn, if it can: Claude
/// Code checkpoints each turn's files and restores them (`false`); Codex
/// drops the turn from its history and leaves files as they are (`true`),
/// and only for a turn the running process ran itself.
fn undo_keeps_files(binding: &str, session_epoch: Option<u64>, turn_epoch: u64) -> Option<bool> {
    let epoch = session_epoch?;
    if binding == crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID {
        Some(false)
    } else if binding == crate::acp::bindings::codex::CODEX_BINDING_ID && epoch == turn_epoch {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::undo_keeps_files;
    use crate::acp::bindings::{claude::sdk::CLAUDE_SDK_BINDING_ID, codex::CODEX_BINDING_ID};

    #[test]
    fn claude_restores_files_and_codex_rewinds_only_its_own_runtimes_turn() {
        assert_eq!(
            undo_keeps_files(CLAUDE_SDK_BINDING_ID, Some(2), 1),
            Some(false)
        );
        assert_eq!(undo_keeps_files(CODEX_BINDING_ID, Some(2), 2), Some(true));
        assert_eq!(undo_keeps_files(CODEX_BINDING_ID, Some(2), 1), None);
        assert_eq!(undo_keeps_files(CODEX_BINDING_ID, None, 1), None);
        assert_eq!(undo_keeps_files(CLAUDE_SDK_BINDING_ID, None, 1), None);
        assert_eq!(undo_keeps_files("pi-rpc", Some(1), 1), None);
    }
}
