use std::fs;

use uuid::Uuid;

use super::*;
use crate::domain::{RenameChatRequest, WorkbenchTab};

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
        Some("Chat: New chat 2")
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
    assert_eq!(active_before.messages.len(), 1);
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
    assert_eq!(restarted.messages.len(), 1);
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
