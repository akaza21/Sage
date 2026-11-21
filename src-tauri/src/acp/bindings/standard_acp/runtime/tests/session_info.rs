use super::*;
use sage_acp_protocol::intent::session_event::SessionScopedEvent;

fn session_info(
    fixture: &mut Fixture,
    session: &str,
    mut update: Value,
) -> Result<crate::acp::bindings::adapter::AdapterBatch, AdapterRuntimeError> {
    update["sessionUpdate"] = json!("session_info_update");
    fixture.runtime.ingest(
        &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId": session, "update": update
        }})),
        &Mapper,
    )
}

fn title(fixture: &mut Fixture, raw: &str) {
    let batch = session_info(fixture, SESSION, json!({"title": raw})).unwrap();
    let [
        AdapterEvent::Session {
            id,
            event: SessionScopedEvent::Title { title: Some(title) },
        },
    ] = batch.events()
    else {
        panic!("expected a session title, got {:?}", batch.events());
    };
    assert_eq!(id.external_id.as_str(), SESSION);
    assert_eq!(id.turn_id, None);
    assert_eq!(title.as_str(), raw);
    assert!(batch.egress().is_empty());
}

#[test]
fn title_updates_are_session_scoped_before_during_and_after_a_turn() {
    let mut fixture = running("session-title");
    title(&mut fixture, "Inspect the cache");
    let prompt = start_turn(&mut fixture);
    title(&mut fixture, "Refactor the cache");
    fixture
        .runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":prompt,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
    title(&mut fixture, "Cache refactor complete");
}

#[test]
fn omitted_title_is_metadata_only_and_null_is_an_explicit_clear() {
    let mut fixture = running("session-title-clear");
    title(&mut fixture, "Inspect the cache");
    let batch = session_info(&mut fixture, SESSION, json!({"updatedAt":"2026-11-21"})).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
    let batch = session_info(&mut fixture, SESSION, json!({"title":null})).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Session {
        id, event: SessionScopedEvent::Title { title: None }
    }] if id.external_id.as_str() == SESSION && id.turn_id.is_none()));
    let batch = session_info(&mut fixture, SESSION, json!({"title":"\n"})).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
}

#[test]
fn foreign_session_and_non_string_titles_are_rejected_even_when_idle() {
    let mut fixture = running("foreign-session-title");
    for update in [
        json!({"title":"Other chat"}),
        json!({"title":null}),
        json!({}),
    ] {
        assert!(matches!(
            session_info(&mut fixture, "foreign", update),
            Err(AdapterRuntimeError::Frame)
        ));
    }
    assert!(matches!(
        session_info(&mut fixture, SESSION, json!({"title":7})),
        Err(AdapterRuntimeError::Frame)
    ));
    title(&mut fixture, "Still the right session");
}
