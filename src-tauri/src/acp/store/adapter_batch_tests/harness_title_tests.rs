use super::*;
use crate::acp::intent::session_event::{SessionScopedEvent, fixtures::title};

fn session(event: SessionScopedEvent, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand { event, now_millis })
}

/// A fixture chat with Sage's placeholder title and an attached tab.
fn untitled_chat() -> Fixture {
    let fixture = fixture();
    fixture
        .connection
        .execute_batch(
            "CREATE TABLE workbench_tabs(id TEXT PRIMARY KEY,project_id TEXT,kind TEXT,title TEXT,updated_at INTEGER);
             CREATE TABLE chat_tabs(project_id TEXT,chat_id TEXT,tab_id TEXT);
             INSERT INTO workbench_tabs VALUES('tab-one','project-one','chat','Chat: Chat',0);
             INSERT INTO chat_tabs VALUES('project-one','chat-one','tab-one');
             UPDATE chats SET title='Chat: Chat',title_source='default';",
        )
        .unwrap();
    fixture
}

fn shown(fixture: &Fixture) -> (String, String, String) {
    fixture
        .connection
        .query_row(
            "SELECT c.title,c.title_source,t.title FROM chats c, workbench_tabs t WHERE c.id='chat-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

#[test]
fn a_committed_harness_title_renames_the_chat_and_its_tab() {
    let mut fixture = untitled_chat();
    let before = projection(&fixture.connection);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![session(title("Fix the sidebar"), 6)]),
    )
    .unwrap();
    assert_eq!(
        shown(&fixture),
        (
            "Chat: Fix the sidebar".into(),
            "harness".into(),
            "Chat: Fix the sidebar".into()
        )
    );
    assert!(projection(&fixture.connection) > before);
}

#[test]
fn a_user_named_chat_keeps_its_title_while_the_surface_records_the_harness_title() {
    let mut fixture = untitled_chat();
    fixture
        .connection
        .execute_batch("UPDATE chats SET title='Chat: Mine',title_source='user';")
        .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![session(title("Harness name"), 6)]),
    )
    .unwrap();
    let (chat_title, source, _) = shown(&fixture);
    assert_eq!(
        (chat_title.as_str(), source.as_str()),
        ("Chat: Mine", "user")
    );
    let surface = super::super::session_surface::read(
        &fixture.connection,
        fixture.owner.project_id.as_str(),
        fixture.owner.chat_id.as_str(),
        fixture.owner.session_id.as_str(),
    )
    .unwrap()
    .unwrap()
    .0;
    assert_eq!(surface.title.unwrap().as_str(), "Harness name");
}
