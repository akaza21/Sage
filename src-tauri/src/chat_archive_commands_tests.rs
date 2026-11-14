use std::fs;

use uuid::Uuid;

use super::*;
use crate::{domain::ProjectLayoutRequest, persistence::Database};
use sage_acp_protocol::contract::{SageChatId, SageProjectId};

/// A Project with two attached Chats; returns the state, the first Chat's
/// archive request and the folder to remove.
fn two_chat_state(label: &str) -> (AppState, ChatLifecycleRequest, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-chat-command-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    database
        .create_chat(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: initial.tabs,
            active_tab_id: initial.active_tab_id,
            selected_root_id: initial.selected_root_id,
            selected_resource: initial.selected_resource,
        })
        .unwrap();
    let attached = database.bootstrap_snapshot().unwrap();
    let request = ChatLifecycleRequest {
        project_id: project.id.clone(),
        chat_id: first_chat_id,
        current_layout: ProjectLayoutRequest {
            project_id: project.id,
            tabs: attached.tabs,
            active_tab_id: attached.active_tab_id,
            selected_root_id: attached.selected_root_id,
            selected_resource: attached.selected_resource,
        },
    };
    (AppState::new(database), request, root)
}

#[test]
fn native_archive_and_restore_are_epoch_checked_transitions() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, request, root) = two_chat_state("archive");
        let project_id = request.project_id.clone();
        let first_chat_id = request.chat_id.clone();
        state
            .settings
            .mark_chat_read(
                &crate::settings_store::ChatReadMark {
                    chat_id: first_chat_id.clone(),
                    read_through: 5_000,
                },
                &[],
            )
            .unwrap();

        let browser_project = SageProjectId::parse(project_id.clone()).unwrap();
        let browser_chat = SageChatId::parse(first_chat_id.clone()).unwrap();
        state
            .acp
            .attach_browser_tab(
                browser_project.clone(),
                browser_chat.clone(),
                "browser-7",
                "https://example.com",
                1,
            )
            .unwrap();
        let browser_lease = state
            .acp
            .browser_attachment(&browser_project, &browser_chat)
            .unwrap();

        state
            .acp
            .file_stages
            .lock()
            .unwrap()
            .stage(
                &project_id,
                &first_chat_id,
                crate::acp::attached_context::files::attachment(
                    &state.database,
                    &project_id,
                    &first_chat_id,
                )
                .unwrap()
                .unwrap(),
                None,
                crate::acp::attached_context::files::Resources(std::sync::Arc::new(vec![
                    crate::acp::attached_context::files::Resource {
                        id: "file-archive".into(),
                        name: "notes.txt".into(),
                        bytes: b"context".to_vec(),
                    },
                ])),
            )
            .unwrap();
        let archived = archive_chat_inner(&state, request.clone(), 0).await;
        assert!(
            state
                .acp
                .file_stages
                .lock()
                .unwrap()
                .get(&project_id, &first_chat_id)
                .0
                .is_empty()
        );
        assert!(archived.committed);
        assert!(state.settings.chat_read_marks().unwrap().marks.is_empty());
        assert!(!browser_lease.is_live());
        assert!(
            state
                .acp
                .browser_attachment(&browser_project, &browser_chat)
                .is_none()
        );
        assert_eq!(archived.transition_epoch, 1);
        let archived_snapshot = archived.snapshot.unwrap();
        assert_eq!(archived_snapshot.archived_chats[0].id, first_chat_id);
        let restore_request = ChatLifecycleRequest {
            project_id: project_id.clone(),
            chat_id: first_chat_id,
            current_layout: ProjectLayoutRequest {
                project_id,
                tabs: archived_snapshot.tabs,
                active_tab_id: archived_snapshot.active_tab_id,
                selected_root_id: archived_snapshot.selected_root_id,
                selected_resource: archived_snapshot.selected_resource,
            },
        };
        let stale = restore_chat_inner(&state, restore_request.clone(), 0).await;
        assert!(!stale.committed);
        assert_eq!(stale.transition_epoch, 1);
        let restored = restore_chat_inner(&state, restore_request, 1).await;
        assert!(restored.committed);
        assert_eq!(restored.transition_epoch, 2);
        assert!(restored.snapshot.unwrap().archived_chats.is_empty());
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn archive_refuses_a_chat_with_queued_prompts_until_they_are_gone() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, request, root) = two_chat_state("queued");
        let queued = crate::chat_queue::DurableQueuedPrompt {
            id: "queue-1".into(),
            text: "later".into(),
            attached_context: Default::default(),
            binding_id: "binding".into(),
            model_id: None,
            status: crate::chat_queue::QueuedPromptStatus::Queued,
        };
        state
            .database
            .replace_chat_queued_prompts(&request.project_id, &request.chat_id, &[queued])
            .unwrap();

        let refused = archive_chat_inner(&state, request.clone(), 0).await;
        assert!(!refused.committed);
        assert_eq!(
            refused.recovery_error.as_deref(),
            Some("Send or remove this Chat's queued prompts before archiving it")
        );
        assert_eq!(refused.transition_epoch, 0, "a refusal is not a transition");
        assert!(
            state
                .database
                .bootstrap_snapshot()
                .unwrap()
                .archived_chats
                .is_empty()
        );

        state
            .database
            .replace_chat_queued_prompts(&request.project_id, &request.chat_id, &[])
            .unwrap();
        let archived = archive_chat_inner(&state, request, 0).await;
        assert!(archived.committed, "{:?}", archived.recovery_error);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn exit_waits_for_running_archive_releases_within_its_deadline() {
    let releases = ArchiveReleases::default();
    assert!(releases.wait_settled(std::time::Instant::now()));
    releases.begin();
    // Still running at the deadline: exit goes ahead and says so.
    assert!(!releases.wait_settled(std::time::Instant::now()));
    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| {
            releases.wait_settled(std::time::Instant::now() + std::time::Duration::from_secs(60))
        });
        drop(ReleaseRunning(&releases));
        assert!(waiting.join().unwrap(), "the release ending wakes the wait");
    });
}

#[test]
fn startup_forgets_read_marks_of_deleted_chats_only() {
    let (state, request, root) = two_chat_state("marks");
    for chat_id in [request.chat_id.as_str(), "chat-deleted-before-crash"] {
        state
            .settings
            .mark_chat_read(
                &crate::settings_store::ChatReadMark {
                    chat_id: chat_id.into(),
                    read_through: 1,
                },
                &[],
            )
            .unwrap();
    }
    forget_read_marks_of_deleted_chats(&state.database, &state.settings);
    let marks = state.settings.chat_read_marks().unwrap().marks;
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].chat_id, request.chat_id);
    drop(state);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_archived_lookup_refuses_delete_with_a_retry_message() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, request, root) = two_chat_state("lookup");
        state
            .database
            .connection()
            .execute_batch("ALTER TABLE chat_tabs RENAME TO chat_tabs_hidden")
            .unwrap();
        let refused = delete_chat_inner(
            &state,
            ChatDeleteRequest {
                project_id: request.project_id,
                chat_id: request.chat_id,
            },
            0,
        )
        .await;
        assert!(!refused.committed);
        assert_eq!(
            refused.recovery_error.as_deref(),
            Some("Sage could not save this change; try again.")
        );
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn deleting_an_archived_chat_forgets_its_sessions_access_choices() {
    use sage_acp_protocol::access_mode::SessionAccessMode::{AcceptEdits, FullAccess};
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, request, root) = two_chat_state("delete-access");
        let kept_chat = request
            .current_layout
            .tabs
            .iter()
            .map(|tab| tab.id.clone())
            .find(|id| *id != request.chat_id)
            .unwrap();
        for (session, chat) in [
            ("deleted-session", &request.chat_id),
            ("kept-session", &kept_chat),
        ] {
            state
                .database
                .connection()
                .execute(
                    "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,
                     binding_digest,capabilities_json,state,residency,legacy,revision,
                     projection_revision,created_at,updated_at)
                     VALUES(?1,?2,?3,0,'claude-agent-acp',?4,'{}','ready','cold',0,0,0,1,1)",
                    rusqlite::params![session, request.project_id, chat, "a".repeat(64)],
                )
                .unwrap();
        }
        let token = state
            .settings
            .prepare_chat_access("deleted-session", FullAccess, 1)
            .unwrap();
        state
            .settings
            .grant_chat_access("deleted-session", FullAccess, &token, 2)
            .unwrap();
        state
            .settings
            .set_chat_access("kept-session", AcceptEdits, 3)
            .unwrap();
        let archived = archive_chat_inner(&state, request.clone(), 0).await;
        assert!(archived.committed);
        let deleted = delete_chat_inner(
            &state,
            ChatDeleteRequest {
                project_id: request.project_id.clone(),
                chat_id: request.chat_id.clone(),
            },
            archived.transition_epoch,
        )
        .await;
        assert!(deleted.committed);
        // Gone at once, without waiting for the next startup.
        assert_eq!(state.settings.chat_access("deleted-session").unwrap(), None);
        assert_eq!(
            state.settings.chat_access("kept-session").unwrap(),
            Some(AcceptEdits)
        );
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}
