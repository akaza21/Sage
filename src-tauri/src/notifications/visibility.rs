use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct Visibility {
    generation: u64,
    revision: u64,
    document_visible: bool,
    chats: [Option<[u8; 32]>; 2],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VisibilityUpdate {
    pub generation: u64,
    pub revision: u64,
    pub document_visible: bool,
    pub chat_ids: Vec<String>,
}

impl VisibilityUpdate {
    /// The Chats this update shows: none while the window is hidden.
    pub(super) fn shown_chats(&self) -> Vec<crate::acp::contract::SageChatId> {
        self.chat_ids
            .iter()
            .filter(|_| self.document_visible)
            .filter_map(|id| crate::acp::contract::SageChatId::parse(id.clone()).ok())
            .collect()
    }
}

impl Visibility {
    pub(super) fn reset(&mut self) -> u64 {
        self.generation = self.generation.saturating_add(1);
        self.revision = 0;
        self.document_visible = false;
        self.chats = [None; 2];
        self.generation
    }
    pub(super) fn update(&mut self, update: VisibilityUpdate) -> Result<(), String> {
        if update.chat_ids.len() > 2
            || update
                .chat_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 512)
            || update.revision == 0
        {
            return Err("invalid chat visibility".into());
        }
        if update.generation != self.generation || update.revision <= self.revision {
            return Err("stale chat visibility".into());
        }
        self.revision = update.revision;
        self.document_visible = update.document_visible;
        self.chats = [None; 2];
        for (slot, id) in self.chats.iter_mut().zip(update.chat_ids) {
            *slot = Some(Sha256::digest(id.as_bytes()).into());
        }
        Ok(())
    }
    pub(super) fn suppresses(&self, chat: &str, focused: bool) -> bool {
        focused
            && self.document_visible
            && self
                .chats
                .contains(&Some(Sha256::digest(chat.as_bytes()).into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_focused_visible_panes_suppress_and_stale_renderer_updates_cannot_restore_visibility() {
        let mut state = Visibility::default();
        let generation = state.reset();
        let update = |revision, ids: &[&str]| VisibilityUpdate {
            generation,
            revision,
            document_visible: true,
            chat_ids: ids.iter().map(|s| s.to_string()).collect(),
        };
        state.update(update(2, &["a", "b"])).unwrap();
        assert!(state.suppresses("a", true));
        assert!(state.suppresses("b", true));
        assert!(!state.suppresses("c", true));
        assert!(!state.suppresses("a", false));
        assert!(state.update(update(1, &["c"])).is_err());
        assert!(state.update(update(3, &["a", "b", "c"])).is_err());
        state
            .update(VisibilityUpdate {
                document_visible: false,
                ..update(3, &["a"])
            })
            .unwrap();
        assert!(!state.suppresses("a", true));
        state.reset();
        assert!(state.update(update(4, &["a"])).is_err());
        assert!(!state.suppresses("a", true));
    }

    #[test]
    fn an_update_shows_its_chats_only_while_the_window_is_visible() {
        let update = |document_visible| VisibilityUpdate {
            generation: 1,
            revision: 1,
            document_visible,
            chat_ids: vec!["chat-a".into(), "chat-b".into()],
        };
        let shown: Vec<_> = update(true)
            .shown_chats()
            .iter()
            .map(|chat| chat.as_str().to_owned())
            .collect();
        assert_eq!(shown, ["chat-a", "chat-b"]);
        assert!(update(false).shown_chats().is_empty());
    }
}
