use super::*;
use crate::{
    acp::{
        contract::{SageChatId, SageProjectId, TurnState},
        store::load_chat_page,
    },
    chat_attention::{contract::ChatAttentionV1, read_state, store as attention_store},
};

fn summary(database: &Database, project: &str, chat: &str) -> ChatAttentionV1 {
    attention_store::get(&database.connection(), project, &[chat.into()])
        .unwrap()
        .remove(0)
}

#[test]
fn simulator_repeated_completion_attention_survives_reopen() {
    let base = std::env::temp_dir().join(format!("sage-simulator-attention-{}", Uuid::new_v4()));
    let root = base.join("project");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    assert_eq!(
        summary(&database, &project.id, &chat)
            .activity_position
            .get(),
        0
    );

    let first_turn = database
        .append_simulator_turn_for_model(&chat, "sim-deep", "first", "completed")
        .unwrap();
    let first = summary(&database, &project.id, &chat);
    assert_eq!(first.activity_position.get(), 1);
    assert_eq!(first.terminal_position, Some(first.activity_position));
    assert_eq!(first.terminal_state, Some(TurnState::Completed));
    assert_eq!(first.read_position, None);
    let acknowledged = read_state::ack(
        &mut database.connection(),
        &project.id,
        &chat,
        first.activity_position,
    )
    .unwrap();
    assert_eq!(acknowledged.read_position, Some(first.activity_position));

    let second_turn = database
        .append_simulator_turn_for_model(&chat, "sim-deep", "second", "completed")
        .unwrap();
    assert_eq!(second_turn.session_id, first_turn.session_id);
    let second = summary(&database, &project.id, &chat);
    assert_eq!(second.activity_position.get(), 2);
    assert_eq!(second.terminal_position, Some(second.activity_position));
    assert_eq!(second.read_position, Some(first.activity_position));
    assert_eq!(second.revision.get(), 3);
    assert!(!second.running);
    drop(database);

    let reopened = Database::open(&path).unwrap();
    assert_eq!(summary(&reopened, &project.id, &chat), second);
    let page = load_chat_page(
        &reopened.connection(),
        SageProjectId::parse(&project.id).unwrap(),
        SageChatId::parse(&chat).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        page.observed_attention_position,
        Some(second.activity_position)
    );
    assert_eq!(page.turns.as_slice().len(), 2);
    assert!(
        page.turns
            .as_slice()
            .iter()
            .all(|turn| turn.state == TurnState::Completed)
    );
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}
