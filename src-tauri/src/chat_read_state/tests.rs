use std::{cell::Cell, fs, path::PathBuf};

use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::chat_archive::ChatArchiveStore;
use crate::chat_lifecycle::store::ChatLifecycleStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use crate::{
    project_registry::MAX_AGENT_RAIL_CHATS_PER_PROJECT,
    settings_store::{ChatReadMark, MAX_CHAT_READ_MARKS},
};
use sage_domain::chat::{ChatLifecycleRequest, ProjectLayoutRequest};

struct Fixture {
    base: PathBuf,
    database: Database,
    settings: SettingsStore,
    /// Seeding a chat's Turns makes it the most recently updated chat, so
    /// the rail's recency order follows seeding order.
    updates: Cell<i64>,
}

/// Later than any real creation time, so seeded chats outrank the
/// Turn-less first chat each project starts with.
const SEEDED_UPDATE_BASE: i64 = 4_000_000_000_000;

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-chat-attention-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        Self {
            base,
            database: crate::database::open_database(std::path::Path::new(":memory:")).unwrap(),
            settings: SettingsStore::in_memory().unwrap(),
            updates: Cell::new(SEEDED_UPDATE_BASE),
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
        self.updates.set(self.updates.get() + 1);
        connection
            .execute(
                "UPDATE chats SET updated_at=?3 WHERE project_id=?1 AND id=?2",
                params![project, chat, self.updates.get()],
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
        mark_read(
            &self.database,
            &self.settings,
            &ChatReadMark {
                chat_id: chat.into(),
                read_through,
            },
        )
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

/// Background projects, then an active one, so every background project
/// takes chats without a layout.
fn background_projects(fixture: &Fixture, count: usize) -> Vec<String> {
    let projects = (0..count)
        .map(|index| fixture.project(&format!("project-{index}")).0)
        .collect();
    fixture.project("active");
    projects
}

#[test]
fn only_the_chats_the_rail_shows_are_considered() {
    let fixture = Fixture::new();
    let project = background_projects(&fixture, 1).remove(0);
    let rail = MAX_AGENT_RAIL_CHATS_PER_PROJECT as usize;
    // Seeded in tab order, so the manual order shows the first chats and
    // the recency order the last; the two just past both cuts are hidden.
    let chats = (0..2 * rail + 1)
        .map(|finished| {
            let chat = fixture.background_chat(&project);
            fixture.turns(&project, &chat, &[("completed", Some(finished as i64))]);
            chat
        })
        .collect::<Vec<_>>();
    let listed = fixture
        .attention()
        .chats
        .into_iter()
        .map(|chat| chat.chat_id)
        .collect::<Vec<_>>();
    // The project's first chat has no Turn and takes one manual slot.
    let hidden = &chats[rail - 1..=rail];
    assert_eq!(listed.len(), chats.len() - hidden.len());
    assert!(hidden.iter().all(|chat| !listed.contains(chat)));
    assert!(listed.contains(&chats[0]));
    assert!(listed.contains(&chats[2 * rail]));
}

#[test]
fn every_chat_of_the_open_project_is_considered() {
    let fixture = Fixture::new();
    let (project, first) = fixture.project("open");
    fixture.turns(&project, &first, &[("completed", Some(1))]);
    let layout = fixture.database.bootstrap_snapshot().unwrap();
    let mut layout = ProjectLayoutRequest {
        project_id: project.clone(),
        tabs: layout.tabs,
        active_tab_id: layout.active_tab_id,
        selected_root_id: layout.selected_root_id,
        selected_resource: layout.selected_resource,
    };
    // Enough chats that the first is past both preview cuts.
    for finished in 0..2 * MAX_AGENT_RAIL_CHATS_PER_PROJECT {
        let chat = fixture.database.create_chat(&layout).unwrap();
        fixture.turns(&project, &chat, &[("completed", Some(finished + 2))]);
        let snapshot = fixture.database.bootstrap_snapshot().unwrap();
        layout.tabs = snapshot.tabs;
        layout.active_tab_id = snapshot.active_tab_id;
    }
    fixture
        .database
        .connection()
        .execute(
            "UPDATE workbench_tabs SET position = position + 100 WHERE id = ?1",
            [&first],
        )
        .unwrap();
    assert!(
        fixture
            .attention()
            .chats
            .contains(&unread(&project, &first, 1))
    );
}

#[test]
fn the_list_is_capped_and_keeps_approvals_ahead_of_the_cut() {
    let fixture = Fixture::new();
    let rail = MAX_AGENT_RAIL_CHATS_PER_PROJECT as usize;
    // Enough projects that their rail chats outnumber the answer's cap.
    let projects = background_projects(&fixture, MAX_CHAT_ATTENTION / rail + 1);
    let mut finished = 0_i64;
    for project in &projects {
        for _ in 0..rail {
            let chat = fixture.background_chat(project);
            finished += 1;
            fixture.turns(project, &chat, &[("completed", Some(finished))]);
        }
    }
    let approval = fixture.background_chat(&projects[1]);
    fixture.turns(&projects[1], &approval, &[("waitingApproval", None)]);

    let list = fixture.attention();
    assert!(list.truncated);
    assert_eq!(list.chats.len(), MAX_CHAT_ATTENTION);
    assert_eq!(list.chats[0], awaiting(&projects[1], &approval));
    let finishes = list
        .chats
        .iter()
        .filter_map(|chat| match chat.reason {
            AttentionReason::Unread { finished_at } => Some(finished_at),
            AttentionReason::AwaitingApproval => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(finishes.len(), MAX_CHAT_ATTENTION - 1);
    // Newest finishes first, and the cut takes the oldest.
    assert!(finishes.windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(finishes[0], finished);
}

#[test]
fn reading_many_other_chats_never_evicts_a_chat_the_rail_shows() {
    let fixture = Fixture::new();
    // A chat read in a project that then sits idle.
    let idle = background_projects(&fixture, 1).remove(0);
    let first = fixture.background_chat(&idle);
    fixture.turns(&idle, &first, &[("completed", Some(1))]);
    fixture.read(&first, 1);
    // Then more chats than the mark bound are read elsewhere, most of them
    // past their project's rail cut.
    let per_project = 60;
    let projects = background_projects(&fixture, MAX_CHAT_READ_MARKS / per_project + 1);
    let mut finished = 1_i64;
    for project in &projects {
        for _ in 0..per_project {
            let chat = fixture.background_chat(project);
            finished += 1;
            fixture.turns(project, &chat, &[("completed", Some(finished))]);
            fixture.read(&chat, finished);
        }
    }
    let marks = fixture.settings.chat_read_marks().unwrap().marks;
    assert_eq!(marks.len(), MAX_CHAT_READ_MARKS);
    assert!(marks.iter().any(|mark| mark.chat_id == first));
    // Every chat the rail shows still has its mark.
    assert!(fixture.attention().chats.is_empty());
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
