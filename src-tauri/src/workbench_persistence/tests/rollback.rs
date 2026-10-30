use super::*;
use rusqlite::types::Value;

// Raw rows supplement the public restore assertions: restoration can filter
// corrupt rows, so it alone cannot prove that no staged/partial row survived.
fn durable_rows(database: &Database, sql: &str, project_id: &str) -> Vec<Vec<Value>> {
    let connection = database.connection();
    let mut statement = connection.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([project_id], |row| {
            (0..columns).map(|column| row.get(column)).collect()
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn popup_layout_failure_after_staging_and_upserts_rolls_back_every_row() {
    let (database, project_id, chat_id) = database_with_project();
    let initial = database.bootstrap_snapshot().unwrap();
    assert_eq!(initial.tabs.len(), 1);
    assert_eq!(initial.tabs[0].position, 0);
    let mut request = ProjectLayoutRequest {
        project_id,
        tabs: initial.tabs,
        active_tab_id: initial.active_tab_id,
        selected_root_id: initial.selected_root_id,
        selected_resource: initial.selected_resource,
    };
    for (id, position) in [
        ("rollback-opener", 1),
        ("rollback-following", 2),
        ("rollback-tail", 3),
    ] {
        let mut browser = request.tabs[0].clone();
        browser.id = id.into();
        browser.kind = "browser".into();
        browser.title = id.into();
        browser.position = position;
        request.tabs.push(browser);
    }
    database.persist_project_layout(&request).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let tabs_sql = "SELECT rowid,* FROM workbench_tabs WHERE project_id=?1 ORDER BY id";
    let layout_sql = "SELECT rowid,* FROM project_layouts WHERE project_id=?1";
    let chat_tabs_sql = "SELECT * FROM chat_tabs WHERE project_id=?1 ORDER BY tab_id";
    let original_rows = durable_rows(&database, tabs_sql, &request.project_id);
    let original_layout = durable_rows(&database, layout_sql, &request.project_id);
    let original_chat_tabs = durable_rows(&database, chat_tabs_sql, &request.project_id);

    let mut popup = request.tabs[1].clone();
    popup.id = "rollback-popup".into();
    popup.title = "Popup".into();
    request.tabs.insert(2, popup);
    for (position, tab) in request.tabs.iter_mut().enumerate() {
        tab.position = position as i64;
    }
    request.tabs[1].title = "Updated opener".into();
    request.active_tab_id = Some("rollback-popup".into());

    // Fail a real later ON CONFLICT UPDATE, only after the staged tail,
    // inserted popup, and shifted following tab are visible in this transaction.
    // RAISE(ABORT) aborts this statement, leaving the save boundary responsible
    // for rolling back the transaction's earlier successful statements.
    database
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER fail_popup_layout_tail
         BEFORE UPDATE OF position ON workbench_tabs
         WHEN NEW.id='rollback-tail' AND NEW.position=4
         BEGIN
           SELECT CASE WHEN OLD.position < 0
             AND EXISTS(SELECT 1 FROM workbench_tabs
                        WHERE id='rollback-popup' AND project_id=NEW.project_id AND position=2)
             AND EXISTS(SELECT 1 FROM workbench_tabs
                        WHERE id='rollback-following' AND project_id=NEW.project_id AND position=3)
             AND EXISTS(SELECT 1 FROM workbench_tabs
                        WHERE id='rollback-opener' AND project_id=NEW.project_id
                          AND title='Updated opener' AND position=1)
             THEN RAISE(ABORT,'fixture popup failure after staging and upserts')
             ELSE RAISE(ABORT,'fixture popup failure reached without required prior writes')
           END;
         END;",
        )
        .unwrap();

    let error = database.persist_project_layout(&request).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixture popup failure after staging and upserts"),
        "must reach the injected post-staging failure, got: {error}"
    );
    assert_eq!(
        durable_rows(&database, tabs_sql, &request.project_id),
        original_rows
    );
    assert_eq!(
        durable_rows(&database, layout_sql, &request.project_id),
        original_layout
    );
    assert_eq!(
        durable_rows(&database, chat_tabs_sql, &request.project_id),
        original_chat_tabs
    );
    let restored = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(&restored.tabs).unwrap(),
        serde_json::to_value(&before.tabs).unwrap()
    );
    assert_eq!(restored.active_tab_id, before.active_tab_id);
    assert_eq!(restored.selected_root_id, before.selected_root_id);
    assert_eq!(
        serde_json::to_value(&restored.selected_resource).unwrap(),
        serde_json::to_value(&before.selected_resource).unwrap()
    );
    assert!(restored.tabs.iter().all(|tab| tab.position >= 0));
    assert_eq!(restored.tabs.len(), 4);
    assert!(!restored.tabs.iter().any(|tab| tab.id == "rollback-popup"));

    database
        .connection()
        .execute_batch("DROP TRIGGER fail_popup_layout_tail;")
        .unwrap();
    // The corrected request and its replay must remain usable after rollback.
    for _ in 0..2 {
        database.persist_project_layout(&request).unwrap();
        let restored = database.bootstrap_snapshot().unwrap();
        assert_eq!(restored.active_tab_id.as_deref(), Some("rollback-popup"));
        assert_eq!(
            restored
                .tabs
                .iter()
                .map(|tab| (tab.id.as_str(), tab.position))
                .collect::<Vec<_>>(),
            vec![
                (chat_id.as_str(), 0),
                ("rollback-opener", 1),
                ("rollback-popup", 2),
                ("rollback-following", 3),
                ("rollback-tail", 4),
            ]
        );
        assert_eq!(
            durable_rows(&database, tabs_sql, &request.project_id).len(),
            5
        );
    }
}
