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
        // Claude Code checkpoints each turn's files, so a running Claude
        // session can rewind its latest turn once that turn has finished.
        let rewinds = session.runtime_epoch.is_some()
            && session.binding_id.as_str()
                == crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID;
        view.undo_turn = page
            .turns
            .as_slice()
            .iter()
            .max_by_key(|turn| (turn.created_at, turn.ordinal))
            .filter(|turn| rewinds && turn.session_id == session.id && turn.terminal_at.is_some())
            .map(|turn| turn.id.clone());
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
