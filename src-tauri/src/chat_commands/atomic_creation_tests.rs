use super::*;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_domain::project::ProjectSwitchRequest;
use std::fs;

fn layout(database: &Database) -> ProjectLayoutRequest {
    let snapshot = database.bootstrap_snapshot().unwrap();
    ProjectLayoutRequest {
        project_id: snapshot.project.unwrap().id,
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    }
}

#[test]
fn round3_cross_project_creation_rolls_back_and_reports_both_validation_and_switch_failures() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for failure in ["source layout", "switch write", "switch commit"] {
            let root = std::env::current_dir().unwrap().join("work")
                .join(format!("chat-create-atomic-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("source")).unwrap();
            fs::create_dir_all(root.join("target")).unwrap();
            let database = crate::database::open_database(&root.join("sage.db")).unwrap();
            let source = database.create_project_with_chat(&root.join("source")).unwrap();
            let target = database.create_project_with_chat(&root.join("target")).unwrap();
            let target_layout = layout(&database);
            database.switch_project(&ProjectSwitchRequest {
                target_project_id: source.id.clone(), current_layout: target_layout,
            }).unwrap();
            let valid = layout(&database);
            let target_before: String = database.connection().query_row(
                "SELECT active_tab_id FROM project_layouts WHERE project_id=?1", [&target.id], |row| row.get(0),
            ).unwrap();
            let mut invalid = valid.clone();
            if failure == "source layout" {
                invalid.active_tab_id = Some("missing-chat".into());
            } else if failure == "switch write" {
                database.connection().execute_batch(
                    "CREATE TEMP TRIGGER refuse_project_switch BEFORE UPDATE OF active_project_id ON app_state
                     WHEN NEW.active_project_id IS NOT OLD.active_project_id
                     BEGIN SELECT RAISE(ABORT, 'injected switch write failure'); END;"
                ).unwrap();
            } else {
                database.connection().execute_batch(
                    "CREATE TABLE injected_commit_failure(project_id TEXT REFERENCES projects(id) DEFERRABLE INITIALLY DEFERRED);
                     CREATE TEMP TRIGGER refuse_project_switch AFTER UPDATE OF active_project_id ON app_state
                     WHEN NEW.active_project_id IS NOT OLD.active_project_id
                     BEGIN INSERT INTO injected_commit_failure VALUES ('missing-project'); END;"
                ).unwrap();
            }
            let state = AppState::new(database);
            let chat = uuid::Uuid::new_v4().to_string();
            let result = create_chat_inner(&state, Some(invalid), 0, Some(target.id.clone()), Some(chat.clone())).await;
            assert!(!result.committed, "{failure}");
            assert_eq!(result.transition_epoch, 0);
            assert!(result.snapshot.is_none());
            assert!(result.recovery_error.is_some());
            let snapshot = state.database.bootstrap_snapshot().unwrap();
            assert_eq!(snapshot.project.unwrap().id, source.id);
            assert_eq!(snapshot.active_tab_id, valid.active_tab_id);
            assert_eq!(state.database.connection().query_row("SELECT COUNT(*) FROM chats WHERE id=?1", [&chat], |row| row.get::<_, i64>(0)).unwrap(), 0, "{failure}");
            assert_eq!(state.database.connection().query_row("SELECT active_tab_id FROM project_layouts WHERE project_id=?1", [&target.id], |row| row.get::<_, String>(0)).unwrap(), target_before);
            state.database.connection().execute_batch("DROP TRIGGER IF EXISTS refuse_project_switch; DROP TABLE IF EXISTS injected_commit_failure;").unwrap();
            let retry = create_chat_inner(&state, Some(valid), 0, Some(target.id.clone()), Some(chat.clone())).await;
            assert!(retry.committed, "{failure}: {:?}", retry.recovery_error);
            assert_eq!(retry.transition_epoch, 1);
            let snapshot = retry.snapshot.unwrap();
            assert_eq!(snapshot.project.unwrap().id, target.id);
            assert_eq!(snapshot.active_tab_id.as_deref(), Some(chat.as_str()));
            assert_eq!(state.database.connection().query_row("SELECT COUNT(*) FROM chats", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
            drop(state);
            fs::remove_dir_all(root).unwrap();
        }
    });
}
