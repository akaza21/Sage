use std::{fs, path::Path};

use uuid::Uuid;

use super::search_project_chats_inner;
use crate::{chat_search::ProjectChatSearchRequest, persistence::Database, state::AppState};

fn root(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("sage-chat-search-{label}-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn command_searches_a_non_active_project_without_switching() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let first_root = root("first");
        let second_root = root("second");
        let database = Database::open(Path::new(":memory:")).unwrap();
        let first = database.create_project_from_root(&first_root).unwrap();
        let state = AppState::new(database.clone());
        let request = ProjectChatSearchRequest {
            project_id: first.id.clone(),
            project_generation: 8,
            expected_transition_epoch: 0,
            query: "Chat".into(),
        };

        let response = search_project_chats_inner(&state, request.clone())
            .await
            .unwrap();
        assert_eq!(response.project_id, first.id);
        assert_eq!(response.project_generation, 8);
        assert_eq!(response.transition_epoch, 0);

        *state.project_transitions.lock().await = 1;
        let stale = search_project_chats_inner(&state, request.clone())
            .await
            .unwrap_err();
        assert!(stale.to_string().contains("project changed"));

        *state.project_transitions.lock().await = 0;
        let second = database.create_project_from_root(&second_root).unwrap();
        assert_eq!(
            database.active_project_id().unwrap().as_deref(),
            Some(second.id.as_str())
        );
        let foreign = search_project_chats_inner(&state, request.clone())
            .await
            .unwrap();
        assert_eq!(foreign.project_id, first.id);
        assert!(!foreign.results.is_empty());
        assert_eq!(
            database.active_project_id().unwrap().as_deref(),
            Some(second.id.as_str())
        );
        let missing = search_project_chats_inner(
            &state,
            ProjectChatSearchRequest {
                project_id: "missing-project".into(),
                ..request
            },
        )
        .await
        .unwrap_err();
        assert!(missing.to_string().contains("does not belong"));
        fs::remove_dir_all(first_root).unwrap();
        fs::remove_dir_all(second_root).unwrap();
    });
}
