use super::*;
use crate::acp::contract::{ClientIntentId, RequiredText, SageAgentSessionId, SageChatId};
use crate::acp::store::{StartSessionCommand, load_chat_page, start_session};
use crate::acp_protocol::access_mode::SessionAccessMode::{Ask, ReadOnly, WorkspaceWrite};

struct Chat {
    root: std::path::PathBuf,
    database: Database,
    settings: SettingsStore,
    project: String,
    chat: String,
}

impl Chat {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-access-lineage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap().id;
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        Self {
            root,
            database,
            settings: SettingsStore::in_memory().unwrap(),
            project,
            chat,
        }
    }

    /// Starts the chat's next session on `binding` through the store, as a
    /// send does once the chat has no session head.
    fn start(&self, binding: &str) -> SageAgentSessionId {
        let project = crate::acp::contract::SageProjectId::parse(&self.project).unwrap();
        let chat = SageChatId::parse(&self.chat).unwrap();
        let page = load_chat_page(
            &self.database.connection(),
            project.clone(),
            chat.clone(),
            None,
        )
        .unwrap();
        start_session(
            &mut self.database.connection(),
            StartSessionCommand {
                project_id: project,
                chat_id: chat,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: RequiredText::new("bindingId", binding).unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
        )
        .unwrap()
        .session_id
    }

    /// The session fails mid-turn: recovery marks it failed and removes the
    /// chat's head, so the next send starts a successor.
    fn fail(&self, session: &SageAgentSessionId) {
        let connection = self.database.connection();
        connection
            .execute(
                "UPDATE agent_sessions SET state='failed',terminal_at=2 WHERE id=?1",
                [session.as_str()],
            )
            .unwrap();
        connection
            .execute(
                "DELETE FROM chat_session_heads WHERE session_id=?1",
                [session.as_str()],
            )
            .unwrap();
    }

    fn choice(&self, session: &SageAgentSessionId) -> Option<SessionAccessMode> {
        chat_choice(&self.database, &self.settings, session.as_str()).unwrap()
    }
}

impl Drop for Chat {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_session_that_replaces_a_failed_one_keeps_the_chats_access() {
    let chat = Chat::new();
    let failed = chat.start("pi-rpc");
    chat.settings
        .set_chat_access(failed.as_str(), WorkspaceWrite, 1)
        .unwrap();
    chat.fail(&failed);
    let successor = chat.start("pi-rpc");
    assert_eq!(chat.choice(&successor), Some(WorkspaceWrite));
    // It survives every further replacement, and a choice made in the
    // successor is the chat's from then on.
    chat.settings
        .set_chat_access(successor.as_str(), ReadOnly, 2)
        .unwrap();
    chat.fail(&successor);
    let third = chat.start("pi-rpc");
    assert_eq!(chat.choice(&third), Some(ReadOnly));
    assert_eq!(chat.choice(&failed), Some(WorkspaceWrite));
}

#[test]
fn another_harness_in_the_chat_keeps_its_own_access() {
    let chat = Chat::new();
    let pi = chat.start("pi-rpc");
    chat.settings
        .set_chat_access(pi.as_str(), WorkspaceWrite, 1)
        .unwrap();
    chat.fail(&pi);
    let claude = chat.start("claude-agent-acp");
    assert_eq!(chat.choice(&claude), None);
    chat.settings.set_chat_access(claude.as_str(), Ask, 2).unwrap();
    chat.fail(&claude);
    // Back on Pi, the chat's Pi choice applies again.
    let pi_again = chat.start("pi-rpc");
    assert_eq!(chat.choice(&pi_again), Some(WorkspaceWrite));
}
