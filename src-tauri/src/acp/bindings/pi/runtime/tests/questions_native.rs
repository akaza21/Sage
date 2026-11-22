use super::*;

#[test]
#[ignore = "requires the installed Pi package and Node.js"]
fn installed_pi_150_answers_four_dialog_kinds_in_command_turns_and_at_idle() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let crate::harness_discovery::PiLaunchDiscovery::Present(launch) =
        crate::harness_discovery::discover_pi_launch()
    else {
        panic!("Pi must be installed")
    };
    assert_eq!(launch.package_version, "1.5.0");
    for mode in ["command", "idle"] {
        let mut journey = Journey::new(SessionAccessMode::ReadOnly);
        journey.settle();
        let profile = journey.root.join("profile");
        std::fs::create_dir_all(profile.join("extensions")).unwrap();
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
                "questions",
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/testdata/pi/extensions/questions.mjs"
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
            "questions",
        );
        let runtime = &mut journey.runtime;
        let discovery = runtime.discover_commands().unwrap().unwrap();
        let discovery = deliver(runtime, discovery);
        pi.send(discovery.clone());
        for _ in 0..128 {
            let value = pi.next();
            let done = value["id"] == discovery["id"];
            runtime.ingest(&frame(value), &Mapper).unwrap();
            if done {
                break;
            }
        }
        let catalog = runtime.available_commands().snapshot();
        assert!(
            catalog
                .commands
                .iter()
                .any(|command| command.name == "fixture-questions")
        );
        let mut prompt = Prompt::new("prompt", format!("/fixture-questions {mode}")).unwrap();
        prompt.command_revision = Some(catalog.revision);
        let outbound = runtime
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new("client", format!("questions-{mode}"))
                    .unwrap(),
                access_mode: SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let sent = deliver(runtime, outbound);
        pi.send(sent.clone());
        let mut completed = false;
        let mut result = None;
        let mut kinds = Vec::new();
        for step in 0..256 {
            let value = pi.next();
            completed |= value["type"] == "response" && value["id"] == sent["id"];
            if value["method"] == "select" && value["title"] == "Over cap" {
                let mut captured = value.clone();
                captured["id"] = json!("too-many");
                assert_eq!(
                    captured,
                    serde_json::from_slice::<Value>(include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/testdata/acp/pi/select-over-cap-150.json"
                    )))
                    .unwrap()
                );
            }
            if value["method"] == "notify"
                && let Some(message) = value["message"].as_str()
                && message.starts_with('{')
            {
                result = Some(serde_json::from_str::<Value>(message).unwrap());
            }
            let mut batch = runtime.ingest(&frame(value), &Mapper).unwrap();
            if matches!(batch.events(), [AdapterEvent::TurnStarted { .. }]) {
                assert_eq!(mode, "idle");
                batch = runtime
                    .adopt_turn(
                        RequiredText::new("client", format!("idle-{step}")).unwrap(),
                        &Mapper,
                    )
                    .unwrap();
            }
            let (events, egress) = batch.into_parts();
            for reply in egress {
                if reply.token().is_mandatory() {
                    runtime.commit_mandatory_egress(reply.token()).unwrap();
                }
                pi.send(deliver(runtime, reply));
            }
            for event in events {
                if let AdapterEvent::PermissionRequest {
                    external_request_id,
                    payload:
                        NormalizedPayload::PermissionRequest {
                            user_input: Some(form),
                            ..
                        },
                    ..
                } = event
                {
                    let question = &form.questions.as_slice()[0];
                    kinds.push(question.kind);
                    use sage_acp_protocol::intent::user_input::UserInputKind;
                    let decision = match question.kind {
                        UserInputKind::Confirm => PermissionDecision::AllowOnce,
                        UserInputKind::Choice => PermissionDecision::Answer(serde_json::from_value(json!([{"questionId":"answer","selectedOption":format!("Target 49: {}", "x".repeat(100))}])).unwrap()),
                        UserInputKind::Text => PermissionDecision::Answer(serde_json::from_value(json!([{"questionId":"answer","text":"pi-input"}])).unwrap()),
                        UserInputKind::Multiline => PermissionDecision::Answer(serde_json::from_value(json!([{"questionId":"answer","text":"Line 1\nEdited line 2"}])).unwrap()),
                        other => panic!("unexpected kind {other:?}"),
                    };
                    let reply = runtime
                        .command(AdapterCommand::ResolvePermission {
                            request_id: SageServerRequestId::parse(external_request_id.as_str())
                                .unwrap(),
                            decision,
                        })
                        .unwrap();
                    pi.send(deliver(runtime, reply));
                    runtime.answers_delivered(&Mapper).unwrap();
                }
            }
            if completed && result.is_some() {
                break;
            }
        }
        use sage_acp_protocol::intent::user_input::UserInputKind::{
            Choice, Confirm, Multiline, Text,
        };
        assert_eq!(kinds, [Choice, Confirm, Text, Multiline, Text]);
        assert!(completed);
        assert_eq!(
            result.unwrap(),
            json!({"selected":format!("Target 49: {}", "x".repeat(100)), "confirmed":true, "input":"pi-input", "editor":"Line 1\nEdited line 2", "overCapCancelled":true, "after":"pi-input"})
        );
    }
}
