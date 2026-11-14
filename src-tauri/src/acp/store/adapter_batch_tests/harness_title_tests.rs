use super::*;
use crate::acp::{
    bindings::session_title::harness_title_cleared,
    intent::session_event::{SessionScopedEvent, SessionSurface, fixtures::title},
};

fn session(event: SessionScopedEvent, now_millis: i64) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand { event, now_millis })
}

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
    assert_eq!(surface(&fixture).title.unwrap().as_str(), "Harness name");
}

fn surface(fixture: &Fixture) -> SessionSurface {
    super::super::session_surface::read(
        &fixture.connection,
        fixture.owner.project_id.as_str(),
        fixture.owner.chat_id.as_str(),
        fixture.owner.session_id.as_str(),
    )
    .unwrap()
    .unwrap()
    .0
}

fn apply(fixture: &mut Fixture, event: SessionScopedEvent, now_millis: i64) {
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![session(event, now_millis)]),
    )
    .unwrap();
}

fn same(title: &str, source: &str) -> (String, String, String) {
    (title.into(), source.into(), title.into())
}

#[test]
fn a_cleared_harness_title_restores_the_first_prompt_title() {
    let mut fixture = untitled_chat();
    apply(&mut fixture, title("Harness name"), 6);
    let before = projection(&fixture.connection);
    apply(&mut fixture, harness_title_cleared(), 7);
    assert_eq!(shown(&fixture), same("Chat: test", "prompt"));
    assert!(projection(&fixture.connection) > before);
    assert_eq!(surface(&fixture).title, None);

    apply(&mut fixture, title("Harness again"), 8);
    assert_eq!(shown(&fixture), same("Chat: Harness again", "harness"));
}

#[test]
fn a_cleared_harness_title_without_a_usable_first_prompt_restores_the_placeholder() {
    let mut fixture = untitled_chat();
    fixture
        .connection
        .execute_batch(
            "UPDATE items SET payload_json='{\"kind\":\"text\",\"payload\":{\"text\":\" \\n\"}}'
             WHERE ordinal=0 AND origin='primary';",
        )
        .unwrap();
    apply(&mut fixture, title("Harness name"), 6);
    apply(&mut fixture, harness_title_cleared(), 7);
    assert_eq!(shown(&fixture), same("Chat: Chat", "default"));
}

#[test]
fn a_cleared_harness_title_leaves_a_user_named_chat_alone() {
    let mut fixture = untitled_chat();
    apply(&mut fixture, title("Harness name"), 6);
    fixture
        .connection
        .execute_batch("UPDATE chats SET title='Chat: Mine',title_source='user';")
        .unwrap();
    apply(&mut fixture, harness_title_cleared(), 7);
    let (chat_title, source, _) = shown(&fixture);
    assert_eq!(
        (chat_title.as_str(), source.as_str()),
        ("Chat: Mine", "user")
    );
    assert_eq!(surface(&fixture).title, None);
}

#[test]
fn clearing_a_session_without_a_title_changes_nothing() {
    let mut fixture = untitled_chat();
    let clear_is_noop = |fixture: &mut Fixture, now_millis| {
        let before = (projection(&fixture.connection), shown(fixture));
        let cleared = apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![session(harness_title_cleared(), now_millis)],
            ),
        )
        .unwrap();
        assert_eq!(cleared.disposition, MutationDisposition::Duplicate);
        assert_eq!((projection(&fixture.connection), shown(fixture)), before);
    };
    clear_is_noop(&mut fixture, 6);
    apply(&mut fixture, title("Harness name"), 7);
    apply(&mut fixture, harness_title_cleared(), 8);
    clear_is_noop(&mut fixture, 9);
}
