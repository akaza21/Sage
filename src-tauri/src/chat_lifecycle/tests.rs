use std::fs;

use uuid::Uuid;

use super::*;
use crate::domain::{ProjectLayoutRequest, RenameChatRequest, WorkbenchTab};

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn requested_creation_identity_survives_reopen_and_cannot_replace_another_chat() {
    let root = std::env::temp_dir().join(format!("sage-chat-identity-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let chat_id = Uuid::new_v4().to_string();
    assert_eq!(
        database
            .create_chat_with_identity(&project.id, &layout(&initial), None, Some(&chat_id))
            .unwrap(),
        chat_id
    );
    database
        .append_simulator_turn(&chat_id, "kept question", "kept answer")
        .unwrap();
    let created = database.bootstrap_snapshot().unwrap();
    let existing_id = initial.active_tab_id.as_deref().unwrap();
    for invalid in [existing_id, chat_id.as_str(), "side-invalid", ""] {
        assert!(
            database
                .create_chat_with_identity(&project.id, &layout(&initial), None, Some(invalid))
                .is_err()
        );
    }
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let restored = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(restored.active_tab_id.as_deref(), Some(chat_id.as_str()));
    assert_eq!(restored.tabs.len(), initial.tabs.len() + 1);
    assert_eq!(
        serde_json::to_value(&restored.tabs).unwrap(),
        serde_json::to_value(&created.tabs).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&restored.messages).unwrap(),
        serde_json::to_value(&created.messages).unwrap()
    );
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rename_preserves_chat_activity_in_the_recent_rail() {
    let root = std::env::temp_dir().join(format!("sage-rename-activity-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let chat_id = initial.active_tab_id.clone().unwrap();
    database
        .append_simulator_turn(&chat_id, "question", "answer")
        .unwrap();
    let mut completed = database.bootstrap_snapshot().unwrap();
    completed
        .tabs
        .iter_mut()
        .find(|tab| tab.id == chat_id)
        .unwrap()
        .status = Some("complete".into());
    database
        .persist_project_layout(&layout(&completed))
        .unwrap();
    // A known prior activity time makes the rename observable without a sleep.
    database
        .connection()
        .execute("UPDATE chats SET updated_at=100 WHERE id=?1", [&chat_id])
        .unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let previous = before
        .projects
        .iter()
        .find(|p| p.id == project.id)
        .unwrap()
        .rail_chats
        .iter()
        .find(|c| c.id == chat_id)
        .unwrap();
    assert_eq!(previous.updated_at, Some(100));
    assert_eq!(previous.status.as_deref(), Some("complete"));
    database
        .rename_chat(&RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: chat_id.clone(),
            title: "Renamed task".into(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    let renamed = after
        .projects
        .iter()
        .find(|p| p.id == project.id)
        .unwrap()
        .rail_chats
        .iter()
        .find(|c| c.id == chat_id)
        .unwrap();
    assert_eq!(renamed.title, "Chat: Renamed task");
    assert_eq!(renamed.status, previous.status);
    assert_eq!(
        renamed.updated_at, previous.updated_at,
        "a rename is not new chat activity"
    );
    assert_eq!(after.active_tab_id, before.active_tab_id);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_display_title_is_the_name_the_user_gave_without_the_stored_prefix() {
    let stored = canonical_chat_title("  Fix login  ").unwrap();
    assert_eq!(super::chat_display_title(&stored), "Fix login");
    // A title the user chose to start with the prefix keeps it.
    let doubled = canonical_chat_title("Chat: notes").unwrap();
    assert_eq!(super::chat_display_title(&doubled), "Chat: notes");
    assert_eq!(super::chat_display_title("Untitled"), "Untitled");
}

#[test]
fn chat_title_contract_trims_and_bounds_utf8_bytes() {
    let two_byte = "\u{00e9}";
    assert_eq!(canonical_chat_title("  Focus  ").unwrap(), "Chat: Focus");
    assert_eq!(
        canonical_chat_title("\u{feff}\u{0085}\u{2003} Focus \u{3000}\u{feff}").unwrap(),
        "Chat: Focus"
    );
    assert_eq!(
        canonical_chat_title(&two_byte.repeat(64)).unwrap(),
        format!("Chat: {}", two_byte.repeat(64))
    );
    for empty in ["\u{2003} \t", "\u{feff}", "\u{0085}"] {
        assert!(
            canonical_chat_title(empty)
                .unwrap_err()
                .to_string()
                .contains("not be empty")
        );
    }
    for controlled in ["line\nbreak", "Focus\u{0085}Later"] {
        assert!(
            canonical_chat_title(controlled)
                .unwrap_err()
                .to_string()
                .contains("control characters")
        );
    }
    assert!(canonical_chat_title(&two_byte.repeat(65)).is_err());

    let request = |title: &str| {
        format!(
            r#"{{"projectId":"p","chatId":"c","title":{title},"currentLayout":{{"projectId":"p","tabs":[],"activeTabId":null,"selectedRootId":null,"selectedResource":null}}}}"#
        )
    };
    for malformed in [r#""\ud800""#, r#""\udc00""#] {
        assert!(serde_json::from_str::<RenameChatRequest>(&request(malformed)).is_err());
    }
    let paired = serde_json::from_str::<RenameChatRequest>(&request(r#""\ud83d\ude80""#)).unwrap();
    assert_eq!(paired.title, "🚀");
}

#[test]
fn rename_rejects_an_over_limit_layout_before_searching_for_the_target() {
    let root = std::env::temp_dir().join(format!("sage-chat-rename-bound-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let chat_id = snapshot.active_tab_id.unwrap();
    let template = snapshot.tabs[0].clone();
    let tabs = (0..=crate::workbench_persistence::MAX_RESTORED_TABS)
        .map(|index| WorkbenchTab {
            id: format!("unattached-{index}"),
            position: index as i64,
            ..template.clone()
        })
        .collect();

    let error = database
        .rename_chat(&RenameChatRequest {
            project_id: project.id.clone(),
            chat_id,
            title: "Bounded".into(),
            current_layout: ProjectLayoutRequest {
                project_id: project.id,
                tabs,
                active_tab_id: None,
                selected_root_id: snapshot.selected_root_id,
                selected_resource: snapshot.selected_resource,
            },
        })
        .unwrap_err();

    assert!(matches!(error, AppError::ResourceLimit(_)));
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rename_is_atomic_for_active_and_inactive_chats_and_survives_restart() {
    let root = std::env::temp_dir().join(format!("sage-chat-rename-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database_path = root.join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    let history = database
        .append_simulator_turn(&first_chat_id, "remember this prompt", "preserved answer")
        .unwrap();
    let second_chat_id = database
        .create_chat(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: initial.tabs,
            active_tab_id: initial.active_tab_id,
            selected_root_id: initial.selected_root_id,
            selected_resource: initial.selected_resource,
        })
        .unwrap();
    let created = database.bootstrap_snapshot().unwrap();
    let tab_identity = created
        .tabs
        .iter()
        .map(|tab| (tab.id.clone(), tab.position))
        .collect::<Vec<_>>();
    let durable_counts = database
        .connection()
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM chats),
               (SELECT COUNT(*) FROM agent_sessions),
               (SELECT COUNT(*) FROM items)",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();

    database
        .rename_chat(&RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: first_chat_id.clone(),
            title: "  Inactive focus  ".into(),
            current_layout: layout(&created),
        })
        .unwrap();
    let inactive_renamed = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        inactive_renamed.active_tab_id.as_deref(),
        Some(second_chat_id.as_str())
    );
    assert_eq!(
        inactive_renamed
            .tabs
            .iter()
            .find(|tab| tab.id == first_chat_id)
            .map(|tab| tab.title.as_str()),
        Some("Chat: Inactive focus")
    );
    assert_eq!(
        inactive_renamed
            .tabs
            .iter()
            .find(|tab| tab.id == second_chat_id)
            .map(|tab| tab.title.as_str()),
        Some("Chat: Chat")
    );

    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: inactive_renamed.tabs,
            active_tab_id: Some(first_chat_id.clone()),
            selected_root_id: inactive_renamed.selected_root_id,
            selected_resource: inactive_renamed.selected_resource,
        })
        .unwrap();
    let active_before = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        active_before.session_id.as_deref(),
        Some(history.session_id.as_str())
    );
    assert_eq!(active_before.messages.len(), 2);
    assert_eq!(active_before.messages[0].text, "remember this prompt");
    assert_eq!(active_before.messages[1].text, "preserved answer");
    database
        .rename_chat(&RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: first_chat_id.clone(),
            title: "Active focus".into(),
            current_layout: layout(&active_before),
        })
        .unwrap();
    let renamed = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        renamed.active_tab_id.as_deref(),
        Some(first_chat_id.as_str())
    );
    assert_eq!(renamed.messages.len(), active_before.messages.len());
    assert_eq!(
        renamed
            .messages
            .iter()
            .map(|message| (&message.id, &message.text))
            .collect::<Vec<_>>(),
        active_before
            .messages
            .iter()
            .map(|message| (&message.id, &message.text))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        renamed
            .tabs
            .iter()
            .map(|tab| (tab.id.clone(), tab.position))
            .collect::<Vec<_>>(),
        tab_identity
    );
    assert_eq!(
        renamed
            .tabs
            .iter()
            .find(|tab| tab.id == first_chat_id)
            .map(|tab| tab.title.as_str()),
        Some("Chat: Active focus")
    );
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM chats),
                   (SELECT COUNT(*) FROM agent_sessions),
                   (SELECT COUNT(*) FROM items)",
                [],
                |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?
                )),
            )
            .unwrap(),
        durable_counts
    );

    let mut stale_layout_tabs = renamed.tabs.clone();
    stale_layout_tabs
        .iter_mut()
        .find(|tab| tab.id == first_chat_id)
        .unwrap()
        .title = "Chat: Stale WebView title".into();
    database
        .persist_project_layout(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: stale_layout_tabs,
            active_tab_id: renamed.active_tab_id.clone(),
            selected_root_id: renamed.selected_root_id.clone(),
            selected_resource: renamed.selected_resource.clone(),
        })
        .unwrap();
    assert_eq!(
        database
            .bootstrap_snapshot()
            .unwrap()
            .tabs
            .iter()
            .find(|tab| tab.id == first_chat_id)
            .map(|tab| tab.title.as_str()),
        Some("Chat: Active focus")
    );

    let before_failures = database.bootstrap_snapshot().unwrap();
    let failure_layout = layout(&before_failures);
    let mut changed_failure_layout = failure_layout.clone();
    changed_failure_layout.active_tab_id = Some(second_chat_id.clone());
    changed_failure_layout.tabs.reverse();
    database
        .connection()
        .execute_batch(
            "CREATE TRIGGER fail_chat_title_update
             BEFORE UPDATE OF title ON chats
             BEGIN SELECT RAISE(ABORT, 'injected rename failure'); END;",
        )
        .unwrap();
    assert!(
        database
            .rename_chat(&RenameChatRequest {
                project_id: project.id.clone(),
                chat_id: first_chat_id.clone(),
                title: "Must roll back".into(),
                current_layout: changed_failure_layout,
            })
            .is_err()
    );
    let after_failed_layout = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        after_failed_layout.active_tab_id,
        before_failures.active_tab_id
    );
    assert_eq!(
        after_failed_layout
            .tabs
            .iter()
            .map(|tab| (&tab.id, &tab.title, tab.position))
            .collect::<Vec<_>>(),
        before_failures
            .tabs
            .iter()
            .map(|tab| (&tab.id, &tab.title, tab.position))
            .collect::<Vec<_>>()
    );
    database
        .connection()
        .execute_batch("DROP TRIGGER fail_chat_title_update")
        .unwrap();
    for request in [
        RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: first_chat_id.clone(),
            title: " \t ".into(),
            current_layout: failure_layout.clone(),
        },
        RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: first_chat_id.clone(),
            title: "bad\nname".into(),
            current_layout: failure_layout.clone(),
        },
        RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: first_chat_id.clone(),
            title: "\u{00e9}".repeat(65),
            current_layout: failure_layout.clone(),
        },
        RenameChatRequest {
            project_id: "foreign-project".into(),
            chat_id: first_chat_id.clone(),
            title: "Foreign".into(),
            current_layout: failure_layout.clone(),
        },
        RenameChatRequest {
            project_id: project.id.clone(),
            chat_id: "foreign-chat".into(),
            title: "Foreign".into(),
            current_layout: failure_layout,
        },
    ] {
        assert!(database.rename_chat(&request).is_err());
        assert_eq!(
            database
                .bootstrap_snapshot()
                .unwrap()
                .tabs
                .iter()
                .map(|tab| (tab.id.clone(), tab.title.clone()))
                .collect::<Vec<_>>(),
            before_failures
                .tabs
                .iter()
                .map(|tab| (tab.id.clone(), tab.title.clone()))
                .collect::<Vec<_>>()
        );
    }
    drop(database);

    let reopened = Database::open(&database_path).unwrap();
    let restarted = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(
        restarted.active_tab_id.as_deref(),
        Some(first_chat_id.as_str())
    );
    assert_eq!(restarted.session_id, Some(history.session_id));
    assert_eq!(restarted.messages.len(), 2);
    assert_eq!(restarted.messages[0].text, "remember this prompt");
    assert_eq!(restarted.messages[1].text, "preserved answer");
    assert_eq!(
        restarted
            .tabs
            .iter()
            .find(|tab| tab.id == first_chat_id)
            .map(|tab| tab.title.as_str()),
        Some("Chat: Active focus")
    );
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn renames_a_chat_on_a_non_active_project_without_switching() {
    let base = std::env::temp_dir().join(format!("sage-chat-rename-foreign-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let cross_chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .append_simulator_turn(&cross_chat, "research prompt", "research answer")
        .unwrap();
    let sessions_before: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_sessions WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![cross.id, cross_chat],
            |row| row.get(0),
        )
        .unwrap();
    assert!(sessions_before > 0);
    let sage = database.create_project_from_root(&second_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .rename_chat(&RenameChatRequest {
            project_id: cross.id.clone(),
            chat_id: cross_chat.clone(),
            title: "Research memory".into(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let renamed = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap()
        .rail_chats
        .iter()
        .find(|chat| chat.id == cross_chat)
        .unwrap();
    assert_eq!(renamed.title, "Chat: Research memory");
    let sessions_after: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_sessions WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![cross.id, cross_chat],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sessions_after, sessions_before);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn creates_a_chat_on_a_non_active_project_without_switching() {
    let base = std::env::temp_dir().join(format!("sage-chat-create-foreign-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let seeded = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let sage_layout = layout(&before);
    let created = database.create_chat_on(&cross.id, &sage_layout).unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    assert_eq!(after.active_tab_id, before.active_tab_id);
    assert_eq!(
        after
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>(),
        before
            .tabs
            .iter()
            .map(|tab| tab.id.as_str())
            .collect::<Vec<_>>()
    );
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    assert!(cross_item.rail_chats.iter().any(|chat| chat.id == seeded));
    let created_chat = cross_item
        .rail_chats
        .iter()
        .find(|chat| chat.id == created)
        .unwrap();
    assert_eq!(created_chat.title, "Chat: Chat");
    let sessions: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_sessions WHERE project_id=?1 AND chat_id=?2",
            rusqlite::params![cross.id, created],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(sessions, 0);
    fs::remove_dir_all(base).unwrap();
}
