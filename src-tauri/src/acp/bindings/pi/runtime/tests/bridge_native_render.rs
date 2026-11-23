use super::*;

fn receive(
    runtime: &mut PiRuntime,
    pi: &mut Pi,
    done: impl Fn(&PiRuntime, &Value) -> bool,
) -> Vec<AdapterEvent> {
    let mut events = Vec::new();
    for _ in 0..256 {
        let value = pi.next();
        let (batch, egress) = runtime
            .ingest(&frame(value.clone()), &Mapper)
            .unwrap()
            .into_parts();
        events.extend(batch);
        assert!(events.len() <= 512);
        for send in egress {
            pi.send(deliver(runtime, send));
        }
        if done(runtime, &value) {
            return events;
        }
    }
    panic!("Pi fixture exceeded its frame bound");
}

#[test]
#[ignore = "requires the installed Pi package and Node.js"]
fn installed_pi_renders_extension_messages_and_follows_extension_models() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let crate::harness_discovery::PiLaunchDiscovery::Present(launch) =
        crate::harness_discovery::discover_pi_launch()
    else {
        panic!("Pi must be installed")
    };
    check(&launch);
}

fn check(launch: &crate::harness_discovery::DiscoveredPiLaunch) {
    let root = std::env::temp_dir().join(format!("sage-pi-render-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("profile/extensions")).unwrap();
    let mut j = Journey {
        runtime: fixture_runtime(73),
        root,
    };
    let profile = j.root.join("profile");
    for (name, source) in [
        (
            "approval.js",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/testdata/pi/extensions/approval.mjs"
            ))
            .as_slice(),
        ),
        (
            "render.js",
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/testdata/pi/extensions/render.mjs"
            ))
            .as_slice(),
        ),
    ] {
        std::fs::write(profile.join("extensions").join(name), source).unwrap();
    }
    let mut pi = Pi::start(
        launch,
        &j.root,
        &profile,
        crate::acp::bindings::pi::bridge::install::installed(&launch.package_version).unwrap(),
        "echo",
    );
    let init = j.runtime.command(AdapterCommand::Initialize).unwrap();
    pi.send(deliver(&mut j.runtime, init));
    receive(&mut j.runtime, &mut pi, |runtime, _| runtime.initialized);
    assert!(
        j.runtime
            .available_models
            .iter()
            .any(|model| model.id == "physical" && model.provider == "sage-render"),
        "session_start provider registration must precede discovery"
    );
    assert!(
        j.runtime
            .available_models
            .iter()
            .any(|model| model.id == "virtual" && model.provider == "sage-router")
    );
    let open = j
        .runtime
        .command(AdapterCommand::NewSession {
            native_root: j.root.to_string_lossy().into_owned(),
            model: None,
        })
        .unwrap();
    pi.send(deliver(&mut j.runtime, open));
    receive(&mut j.runtime, &mut pi, |runtime, _| {
        runtime.external_session.is_some()
    });
    let commands = j.runtime.discover_commands().unwrap().unwrap();
    pi.send(deliver(&mut j.runtime, commands));
    receive(&mut j.runtime, &mut pi, |runtime, _| {
        !runtime.commands_pending
    });
    let list = j.runtime.available_commands().snapshot();
    let command = list
        .commands
        .iter()
        .find(|command| command.name == "fixture-render")
        .unwrap();
    let info = command.source_info.as_ref().unwrap();
    assert_eq!(info.scope.as_deref(), Some("user"));
    assert_eq!(info.origin.as_deref(), Some("top-level"));
    assert!(info.path.as_ref().unwrap().ends_with("render.js"));
    // A configuration read waits for the next prompt: the state, then the
    // model registry and thinking levels when the model may have changed.
    let settled = |runtime: &PiRuntime| {
        !runtime.configuration_dirty
            && !runtime.translator.configuration_refresh_pending()
            && runtime.thinking_request_command.is_none()
    };
    let prompts = ["render", "unknown-role"];
    for (index, name) in prompts.into_iter().enumerate() {
        let mut prompt = Prompt::new("prompt", name).unwrap();
        prompt.command_revision = None;
        let send = j
            .runtime
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new("client", format!("render-{index}")).unwrap(),
                access_mode: SessionAccessMode::ReadOnly,
            })
            .unwrap();
        pi.send(deliver(&mut j.runtime, send));
        let events = receive(&mut j.runtime, &mut pi, |runtime, value| {
            runtime.active_turn.is_none() && value["type"] == "agent_settled"
        });
        if name == "render" {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        AdapterEvent::Item {
                            payload: NormalizedPayload::ExtensionMessage { .. },
                            ..
                        }
                    ))
                    .count(),
                1
            );
            assert!(events.iter().any(|event| matches!(event, AdapterEvent::Item { payload: NormalizedPayload::ExtensionMessage { label, text, details: Some(details) }, .. } if label.as_str() == "fixture-report" && text.as_str() == "<script>safe text</script>" && details.as_str().len() <= 16384)));
            // The extension's sendUserMessage shows once; Sage's own prompt
            // echo does not.
            let users = events
                .iter()
                .filter_map(|event| match event {
                    AdapterEvent::Item {
                        payload: NormalizedPayload::UserPrompt { text },
                        ..
                    } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(users, ["Injected request"]);
            // The extension's setModel reached the model option within the
            // turn, before any prompt-time configuration read.
            assert_eq!(
                j.runtime.current_model.as_deref(),
                Some("sage-router:virtual")
            );
        }
        if name != "unknown-role" {
            continue;
        }
        assert!(
            events
                .iter()
                .any(|event| matches!(event, AdapterEvent::Unsupported(_))),
            "unknown augmented roles must be disclosed"
        );
        if !settled(&j.runtime) {
            receive(&mut j.runtime, &mut pi, |runtime, _| settled(runtime));
        }
        // The render turn's setModel and setThinkingLevel.
        assert_eq!(
            j.runtime.current_model.as_deref(),
            Some("sage-router:virtual")
        );
        assert_eq!(j.runtime.current_thinking.as_deref(), Some("low"));
    }
}
