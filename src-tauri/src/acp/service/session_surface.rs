use super::*;
use sage_acp_protocol::scheduler::RuntimeOwner;

impl AcpService {
    /// Live activities and Undo are display decoration: a page they would push
    /// over its byte budget goes without them.
    pub(super) fn compose_session_surface(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        let Some(session) = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| Some(&session.id) == page.current_session_id.as_ref())
        else {
            return Ok(());
        };
        let latest_only = session.capabilities.rewind.targets
            == sage_acp_protocol::intent::RewindTargets::LatestTurn;
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
        let undo = match (latest, session.runtime_epoch) {
            (Some(turn), Some(epoch)) => self
                .turn_undo(
                    &RuntimeOwner {
                        project_id: page.project_id.clone(),
                        chat_id: page.chat_id.clone(),
                        session_id: session.id.clone(),
                        runtime_epoch: epoch,
                    },
                    &session.capabilities,
                    turn.id.as_str(),
                )?
                .map(|undo| (turn.id.clone(), undo.keeps_files)),
            _ => None,
        };
        view.undo_keeps_files = undo.as_ref().is_some_and(|(_, keeps)| *keeps);
        view.undo_turn = undo.as_ref().map(|(turn, _)| turn.clone());
        page.session_surface = (view != Default::default()).then_some(view);
        let bytes = serde_json::to_vec(page)
            .map_err(|_| StoreError::Corrupt("chat page serialization"))?
            .len();
        if bytes > sage_core::limits::LIMITS.durable.history_page_bytes as usize {
            page.session_surface = None;
        } else if latest_only
            && let Some((id, _)) = undo
            && let Some(turn) = page
                .turns
                .as_mut_slice()
                .iter_mut()
                .find(|turn| turn.id == id)
        {
            // A last-turn native checkpoint needs no durable message receipt.
            turn.conversation_target = true;
        }
        Ok(())
    }
}
