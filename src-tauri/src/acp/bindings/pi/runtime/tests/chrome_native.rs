//! Opt-in installed-Pi test: Pi's shipped rpc-demo extension and a fixture
//! past every cap, through the real RPC extension UI.
use super::*;
use sage_acp_protocol::intent::session_event::{
    ChromePlacement, SessionChrome, SessionNotice, SessionScopedEvent,
};

fn session_events(events: Vec<AdapterEvent>) -> impl Iterator<Item = SessionScopedEvent> {
    events.into_iter().filter_map(|event| match event {
        AdapterEvent::Session { event, .. } => Some(event),
        _ => None,
    })
}

fn pump(runtime: &mut PiRuntime, pi: &mut Pi, sent: &Value, events: &mut Vec<SessionScopedEvent>) {
    for _ in 0..256 {
        let value = pi.next();
        let done = value["type"] == "response" && value["id"] == sent["id"];
        let (batch, egress) = runtime.ingest(&frame(value), &Mapper).unwrap().into_parts();
        for request in egress {
            pi.send(deliver(runtime, request));
        }
        events.extend(session_events(batch));
        if done {
            // A burst of chrome changes is sent once its gap has passed.
            if let Some(due) = runtime.next_request_deadline() {
                std::thread::sleep(due.saturating_duration_since(std::time::Instant::now()));
                if let Some(batch) = runtime.poll_request_deadlines(&Mapper).unwrap() {
                    events.extend(session_events(batch.into_parts().0));
                }
            }
            return;
        }
    }
    panic!("bounded Pi command");
}

fn chrome(events: &[SessionScopedEvent]) -> SessionChrome {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            SessionScopedEvent::Chrome(chrome) => Some(chrome.clone()),
            _ => None,
        })
        .expect("a chrome snapshot")
}

fn notices(events: &[SessionScopedEvent]) -> Vec<(Option<String>, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionScopedEvent::Notice(SessionNotice { identity, text, .. }) => Some((
                identity
                    .as_ref()
                    .map(|identity| identity.as_str().to_owned()),
                text.as_str().to_owned(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
#[ignore = "requires the installed Pi package and Node.js"]
fn installed_pi_150_extension_status_widget_editor_text_and_title_are_bounded_chrome() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let crate::harness_discovery::PiLaunchDiscovery::Present(launch) =
        crate::harness_discovery::discover_pi_launch()
    else {
        panic!("Pi must be installed")
    };
    assert_eq!(launch.package_version, "1.5.0");
    let demo = std::path::Path::new(&launch.entrypoint_path)
        .ancestors()
        .map(|dir| dir.join("examples/extensions/rpc-demo.ts"))
        .find(|path| path.is_file())
        .expect("Pi ships examples/extensions/rpc-demo.ts");
    let mut journey = Journey::new(SessionAccessMode::ReadOnly);
    journey.settle();
    let profile = journey.root.join("profile");
    std::fs::create_dir_all(profile.join("extensions")).unwrap();
    std::fs::copy(&demo, profile.join("extensions/rpc-demo.ts")).unwrap();
    for (name, bytes) in [
        (
            "approval",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/testdata/pi/extensions/approval.mjs"
            ))
            .as_slice(),
        ),
        (
            "chrome",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/testdata/pi/extensions/chrome.mjs"
            ))
            .as_slice(),
        ),
    ] {
        std::fs::write(profile.join(format!("extensions/{name}.js")), bytes).unwrap();
    }
    let mut pi = Pi::start(
        &launch,
        &journey.root,
        &profile,
        crate::acp::bindings::pi::bridge::install::installed(&launch.package_version).unwrap(),
        "echo",
    );
    let runtime = &mut journey.runtime;
    let mut events = Vec::new();
    let discovery = runtime.discover_commands().unwrap().unwrap();
    let discovery = deliver(runtime, discovery);
    pi.send(discovery.clone());
    pump(runtime, &mut pi, &discovery, &mut events);
    // rpc-demo's session_start: title, widget and status appear.
    let started = chrome(&events);
    assert_eq!(started.subtitle.as_ref().unwrap().as_str(), "pi RPC Demo");
    let [status] = started.statuses.as_slice() else {
        panic!("one status: {started:?}")
    };
    assert_eq!(
        (status.key.as_str(), status.text.as_str()),
        ("rpc-demo", "Turns: 0")
    );
    let [widget] = started.widgets.as_slice() else {
        panic!("one widget: {started:?}")
    };
    assert_eq!(widget.placement, ChromePlacement::Above);
    assert_eq!(
        widget
            .lines
            .as_slice()
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>(),
        ["--- RPC Extension UI Demo ---", "Loaded and ready."]
    );
    let catalog = runtime.available_commands().snapshot();
    let mut command = |runtime: &mut PiRuntime, text: &str, events: &mut Vec<_>| {
        events.clear();
        let mut prompt = Prompt::new("prompt", text).unwrap();
        prompt.command_revision = Some(catalog.revision);
        let outbound = runtime
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new(
                    "client",
                    text.trim_start_matches('/').replace(' ', "-"),
                )
                .unwrap(),
                access_mode: SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let sent = deliver(runtime, outbound);
        pi.send(sent.clone());
        pump(runtime, &mut pi, &sent, events);
    };
    command(runtime, "/rpc-prefill", &mut events);
    assert_eq!(
        chrome(&events).editor_text.unwrap().text.as_str(),
        "This text was set by the rpc-demo extension."
    );
    assert_eq!(
        notices(&events),
        // Pi names no extension source, so the notice keeps no identity.
        [(None, "Editor prefilled".to_owned())]
    );
    command(runtime, "/fixture-chrome caps", &mut events);
    let capped = chrome(&events);
    // A new prompt answered the earlier offer; the oversize one is refused.
    assert_eq!(capped.editor_text, None);
    let statuses = capped.statuses.as_slice();
    assert_eq!(statuses.len(), 8);
    assert_eq!(statuses[0].key.as_str(), "rpc-demo");
    assert_eq!(
        statuses[1].text.as_str(),
        format!("{}\u{2026}", "y".repeat(197))
    );
    assert!(statuses.iter().all(|status| status.key.as_str() != "s7"));
    let widgets = capped.widgets.as_slice();
    assert_eq!(
        widgets
            .iter()
            .map(|widget| widget.key.as_str())
            .collect::<Vec<_>>(),
        ["rpc-demo", "w0", "w1", "w2"]
    );
    let long = &widgets[1];
    assert_eq!(long.placement, ChromePlacement::Below);
    assert_eq!((long.lines.as_slice().len(), long.hidden_lines), (10, 2));
    assert_eq!(long.lines.as_slice()[0].as_str().len(), 200);
    assert_eq!(
        notices(&events)
            .into_iter()
            .filter_map(|(identity, _)| identity)
            .collect::<Vec<_>>(),
        [
            "pi-extension-ui:status-limit",
            "pi-extension-ui:widget-limit",
            "pi-extension-ui:not-text",
            "pi-extension-ui:editor-text-limit",
        ]
    );
    command(runtime, "/fixture-chrome clear", &mut events);
    assert_eq!(chrome(&events), SessionChrome::default());
}
