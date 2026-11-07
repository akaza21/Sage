use std::{fs, path::PathBuf};

use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::{
    domain::{ChatLifecycleRequest, ProjectLayoutRequest},
    settings_store::ChatReadMark,
};

struct Fixture {
    base: PathBuf,
    database: Database,
    settings: SettingsStore,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-chat-attention-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        Self {
            base,
            database: Database::open(std::path::Path::new(":memory:")).unwrap(),
            settings: SettingsStore::in_memory().unwrap(),
        }
    }

    /// A new project, and its first chat. Each new project becomes active,
    /// so every earlier project is a background one.
    fn project(&self, name: &str) -> (String, String) {
        let root = self.base.join(name);
        fs::create_dir_all(&root).unwrap();
        let project = self.database.create_project_from_root(&root).unwrap();
        let chat = self
            .database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        (project.id, chat)
    }

    /// Another chat on a project that is no longer active. (On the active
    /// project the layout passed in would replace its tabs.)
    fn background_chat(&self, project: &str) -> String {
        self.database
            .create_chat_on(project, &empty_layout(project))
            .unwrap()
    }

    /// Gives the chat one session whose Turns end in the given states, in
    /// order; a terminal Turn finishes at its `terminal_at`.
    fn turns(&self, project: &str, chat: &str, turns: &[(&str, Option<i64>)]) {
        let connection = self.database.connection();
        let session = format!("session-{chat}");
        connection
            .execute(
                "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
                 capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at)
                 VALUES(?3,?1,?2,0,'codex-app-server',?4,'{}','ready','hot',0,0,0,1,1)",
                params![project, chat, session, "a".repeat(64)],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
                 VALUES(?1,?2,?3,1,'running',1)",
                params![project, chat, session],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,
                 item_bytes,fact_count,fact_bytes,payload_bytes,revision)
                 VALUES(?1,?2,?3,?4,0,0,0,0,0,0)",
                params![project, chat, session, turns.len() as i64],
            )
            .unwrap();
        for (ordinal, (state, terminal_at)) in turns.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
                     client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
                     VALUES(?4,?1,?2,?3,1,?5,?4,?6,'correlated',0,1,1,?7)",
                    params![
                        project,
                        chat,
                        session,
                        format!("{chat}-turn-{ordinal}"),
                        ordinal as i64,
                        state,
                        terminal_at
                    ],
                )
                .unwrap();
        }
    }

    fn read(&self, chat: &str, read_through: i64) {
        self.settings
            .mark_chat_read(&ChatReadMark {
                chat_id: chat.into(),
                read_through,
            })
            .unwrap();
    }

    fn attention(&self) -> ChatAttentionList {
        chat_attention(&self.database, &self.settings).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn empty_layout(project: &str) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: project.into(),
        tabs: Vec::new(),
        active_tab_id: None,
        selected_root_id: None,
        selected_resource: None,
    }
}

fn unread(project: &str, chat: &str, finished_at: i64) -> ChatAttention {
    ChatAttention {
        project_id: project.into(),
        chat_id: chat.into(),
        reason: AttentionReason::Unread { finished_at },
    }
}

fn awaiting(project: &str, chat: &str) -> ChatAttention {
    ChatAttention {
        project_id: project.into(),
        chat_id: chat.into(),
        reason: AttentionReason::AwaitingApproval,
    }
}

#[test]
fn lists_unseen_finishes_and_approvals_across_projects() {
    let fixture = Fixture::new();
    let (first, done) = fixture.project("first");
    let (second, approval) = fixture.project("second");
    let failed = fixture.background_chat(&first);
    let running = fixture.background_chat(&first);
    let cancelled = fixture.background_chat(&first);
    let _idle = fixture.background_chat(&first);
    fixture.turns(&first, &done, &[("completed", Some(3_000))]);
    fixture.turns(&first, &failed, &[("failed", Some(4_000))]);
    fixture.turns(
        &first,
        &running,
        &[("completed", Some(1_000)), ("running", None)],
    );
    fixture.turns(&first, &cancelled, &[("cancelled", Some(5_000))]);
    fixture.turns(
        &second,
        &approval,
        &[("completed", Some(2_000)), ("waitingApproval", None)],
    );

    assert_eq!(
        fixture.attention(),
        ChatAttentionList {
            chats: vec![
                awaiting(&second, &approval),
                unread(&first, &failed, 4_000),
                unread(&first, &done, 3_000),
            ],
            truncated: false,
        }
    );
}

#[test]
fn a_read_mark_hides_a_finish_until_a_later_turn_finishes() {
    let fixture = Fixture::new();
    let (project, chat) = fixture.project("read");
    fixture.project("other");
    let seen = fixture.background_chat(&project);
    let behind = fixture.background_chat(&project);
    fixture.turns(&project, &chat, &[("completed", Some(2_000))]);
    fixture.turns(
        &project,
        &seen,
        &[("completed", Some(1_000)), ("completed", Some(2_500))],
    );
    fixture.turns(&project, &behind, &[("completed", Some(3_000))]);
    fixture.read(&chat, 2_000);
    fixture.read(&seen, 1_000);
    fixture.read(&behind, 9_000);

    // Read exactly through its finish, or past it: seen. Read only through
    // an earlier Turn: the later finish is unseen.
    assert_eq!(
        fixture.attention().chats,
        vec![unread(&project, &seen, 2_500)]
    );

    fixture.read(&seen, 2_500);
    assert!(fixture.attention().chats.is_empty());

    // Marking unread brings the seen finish back.
    fixture.settings.mark_chat_unread(&chat).unwrap();
    assert_eq!(
        fixture.attention().chats,
        vec![unread(&project, &chat, 2_000)]
    );
}

#[test]
fn an_approval_shows_even_when_the_chat_was_read() {
    let fixture = Fixture::new();
    let (project, chat) = fixture.project("approval");
    fixture.turns(
        &project,
        &chat,
        &[("completed", Some(1_000)), ("waitingApproval", None)],
    );
    fixture.read(&chat, 9_000);
    assert_eq!(fixture.attention().chats, vec![awaiting(&project, &chat)]);
}

#[test]
fn the_list_is_capped_and_keeps_approvals_ahead_of_the_cut() {
    let fixture = Fixture::new();
    let (first, _) = fixture.project("first");
    let (second, _) = fixture.project("second");
    fixture.project("newest");
    let mut finished = 0_i64;
    for project in [&first, &second] {
        for _ in 0..MAX_CHAT_ATTENTION / 2 {
            let chat = fixture.background_chat(project);
            finished += 1;
            fixture.turns(project, &chat, &[("completed", Some(finished))]);
        }
    }
    // One more unseen chat than the cap, and the approval comes last of all.
    let extra = fixture.background_chat(&first);
    fixture.turns(&first, &extra, &[("completed", Some(0))]);
    let approval = fixture.background_chat(&second);
    fixture.turns(&second, &approval, &[("waitingApproval", None)]);

    let list = fixture.attention();
    assert!(list.truncated);
    assert_eq!(list.chats.len(), MAX_CHAT_ATTENTION);
    assert_eq!(list.chats[0], awaiting(&second, &approval));
    assert_eq!(
        list.chats
            .iter()
            .filter(|chat| matches!(chat.reason, AttentionReason::Unread { .. }))
            .count(),
        MAX_CHAT_ATTENTION - 1
    );
    // The oldest finishes are the ones cut.
    assert!(!list.chats.iter().any(|chat| chat.chat_id == extra));
}

#[test]
fn an_archived_chat_leaves_the_list() {
    let fixture = Fixture::new();
    let (project, chat) = fixture.project("archive");
    fixture.project("other");
    let kept = fixture.background_chat(&project);
    fixture.turns(&project, &chat, &[("completed", Some(1_000))]);
    fixture.turns(&project, &kept, &[("completed", Some(2_000))]);
    fixture
        .database
        .archive_chat(&ChatLifecycleRequest {
            project_id: project.clone(),
            chat_id: chat.clone(),
            current_layout: empty_layout(&project),
        })
        .unwrap();
    assert_eq!(
        fixture.attention().chats,
        vec![unread(&project, &kept, 2_000)]
    );
}

#[test]
fn serializes_the_reason_for_the_port() {
    assert_eq!(
        serde_json::to_value(unread("p", "c", 7)).unwrap(),
        serde_json::json!({
            "projectId": "p",
            "chatId": "c",
            "reason": "unread",
            "finishedAt": 7,
        })
    );
    assert_eq!(
        serde_json::to_value(awaiting("p", "c")).unwrap(),
        serde_json::json!({
            "projectId": "p",
            "chatId": "c",
            "reason": "awaitingApproval",
        })
    );
}
