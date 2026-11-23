use super::*;
use sage_acp_protocol::intent::session_event::{SessionChrome, SessionScopedEvent};

fn session_events(batch: AdapterBatch) -> Vec<SessionScopedEvent> {
    batch
        .into_parts()
        .0
        .into_iter()
        .filter_map(|event| match event {
            AdapterEvent::Session { event, .. } => Some(event),
            _ => None,
        })
        .collect()
}

fn ui(method: &str, fields: Value) -> Value {
    let mut value = json!({"type":"extension_ui_request","id":"ui","method":method});
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    value
}

fn send(runtime: &mut PiRuntime, value: Value) -> Vec<SessionScopedEvent> {
    session_events(runtime.ingest(&frame(value), &Mapper).unwrap())
}

fn notes(events: &[SessionScopedEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionScopedEvent::Notice(notice) => notice.identity.as_ref().map(|id| id.as_str()),
            _ => None,
        })
        .collect()
}

fn flushed(runtime: &mut PiRuntime, clock: &QuestionClock) -> SessionChrome {
    clock.advance_ms(super::super::super::chrome::CHROME_SNAPSHOT_GAP.as_millis() as u64);
    assert_eq!(runtime.next_request_deadline(), Some(clock.now()));
    let events = session_events(runtime.poll_request_deadlines(&Mapper).unwrap().unwrap());
    let [SessionScopedEvent::Chrome(chrome)] = events.as_slice() else {
        panic!("one chrome snapshot: {events:?}")
    };
    chrome.clone()
}

#[test]
fn pi_chrome_waits_for_the_session_and_enforces_its_caps_with_one_note_per_kind() {
    let mut runtime = fixture_runtime(41);
    let clock = QuestionClock::new(&mut runtime);
    let status =
        |key: &str, text: &str| ui("setStatus", json!({"statusKey":key,"statusText":text}));
    // Set while Pi starts up: kept, and reported with the opened session.
    initialize(&mut runtime);
    assert!(send(&mut runtime, status("boot", "Booting")).is_empty());
    assert_eq!(runtime.next_request_deadline(), None);
    let open = runtime
        .command(AdapterCommand::NewSession {
            native_root: "C:/repo".into(),
            model: None,
        })
        .unwrap();
    let open = deliver(&mut runtime, open);
    let batch = runtime.ingest(&response(&open, opened()), &Mapper).unwrap();
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::SessionReady { .. },
            AdapterEvent::Session {
                event: SessionScopedEvent::Chrome(_),
                ..
            }
        ]
    ));
    // A burst inside the gap is merged into one later snapshot.
    let mut events = Vec::new();
    for index in 0..9 {
        events.extend(send(&mut runtime, status(&format!("s{index}"), "Working")));
    }
    events.extend(send(&mut runtime, status("s0", &"y".repeat(300))));
    events.extend(send(&mut runtime, status("extra", "Again")));
    assert_eq!(notes(&events), ["pi-extension-ui:status-limit"]);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SessionScopedEvent::Chrome(_)))
    );
    let chrome = flushed(&mut runtime, &clock);
    let statuses = chrome.statuses.as_slice();
    assert_eq!(statuses.len(), 8);
    assert_eq!(statuses[0].key.as_str(), "boot");
    assert_eq!(
        statuses[1].text.as_str(),
        format!("{}\u{2026}", "y".repeat(197))
    );
    // Widgets: lines past ten are counted, long lines clipped, non-text refused.
    let lines: Vec<String> = (0..12)
        .map(|index| {
            if index == 0 {
                "z".repeat(300)
            } else {
                format!("Line {index}")
            }
        })
        .collect();
    let mut events = send(
        &mut runtime,
        ui(
            "setWidget",
            json!({"widgetKey":"w","widgetLines":lines,"widgetPlacement":"belowEditor"}),
        ),
    );
    events.extend(send(
        &mut runtime,
        ui("setWidget", json!({"widgetKey":"x","widgetLines":[42]})),
    ));
    events.extend(send(
        &mut runtime,
        ui("setWidget", json!({"widgetKey":"y","widgetLines":[1]})),
    ));
    events.extend(send(
        &mut runtime,
        ui("set_editor_text", json!({"text":"x".repeat(17 * 1024)})),
    ));
    events.extend(send(
        &mut runtime,
        ui("set_editor_text", json!({"text":"Try this"})),
    ));
    events.extend(send(&mut runtime, ui("setTitle", json!({"title":"Demo"}))));
    assert_eq!(
        notes(&events),
        [
            "pi-extension-ui:not-text",
            "pi-extension-ui:editor-text-limit"
        ]
    );
    let chrome = flushed(&mut runtime, &clock);
    let [widget] = chrome.widgets.as_slice() else {
        panic!("one widget")
    };
    assert_eq!(
        (widget.lines.as_slice().len(), widget.hidden_lines),
        (10, 2)
    );
    assert_eq!(widget.lines.as_slice()[0].as_str().len(), 200);
    assert_eq!(chrome.editor_text.unwrap().text.as_str(), "Try this");
    assert_eq!(chrome.subtitle.unwrap().as_str(), "Demo");
    // Clearing every kind leaves an empty snapshot; an unchanged repeat sends none.
    for key in ["boot", "s0", "s1", "s2", "s3", "s4", "s5", "s6"] {
        send(&mut runtime, ui("setStatus", json!({"statusKey":key})));
    }
    send(&mut runtime, ui("setWidget", json!({"widgetKey":"w"})));
    send(&mut runtime, ui("set_editor_text", json!({"text":""})));
    send(&mut runtime, ui("setTitle", json!({"title":""})));
    assert_eq!(flushed(&mut runtime, &clock), SessionChrome::default());
    clock.advance_ms(1_000);
    assert!(send(&mut runtime, ui("setTitle", json!({"title":""}))).is_empty());
    assert_eq!(runtime.next_request_deadline(), None);
}
