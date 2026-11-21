//! Opt-in installed-Pi test: real discovery, RPC, blocking hook and Sage adapter.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{Receiver, sync_channel},
    time::Duration,
};

struct Pi {
    child: Child,
    frames: Receiver<Value>,
}
impl Drop for Pi {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Pi {
    fn send(&mut self, value: Value) {
        let input = self.child.stdin.as_mut().unwrap();
        serde_json::to_writer(&mut *input, &value).unwrap();
        input.write_all(b"\n").unwrap();
        input.flush().unwrap();
    }
    fn next(&self) -> Value {
        self.frames
            .recv_timeout(Duration::from_secs(30))
            .expect("Pi RPC frame")
    }
}

#[test]
#[ignore = "requires the installed Pi package and Node.js"]
fn installed_pi_discovers_commands_and_gates_extension_execution_in_both_modes() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let crate::harness_discovery::PiLaunchDiscovery::Present(launch) =
        crate::harness_discovery::discover_pi_launch()
    else {
        panic!("Pi must be installed")
    };
    for scenario in [
        "echo",
        "mutation",
        "collisions",
        "concurrent",
        "overflow",
        "cancel",
    ] {
        for mode in [
            SessionAccessMode::ReadOnly,
            SessionAccessMode::WorkspaceWrite,
        ] {
            for decision in [PermissionDecision::Deny, PermissionDecision::AllowOnce] {
                if ["concurrent", "overflow", "cancel"].contains(&scenario)
                    && (mode != SessionAccessMode::WorkspaceWrite
                        || decision != PermissionDecision::AllowOnce)
                {
                    continue;
                }
                eprintln!("installed Pi: {scenario} {mode:?} {decision:?}");
                let mut j = Journey::new(mode);
                j.settle();
                let profile = j.root.join("profile");
                std::fs::create_dir_all(profile.join("extensions")).unwrap();
                std::fs::write(
                    profile.join("extensions/approval.js"),
                    include_bytes!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/testdata/pi/extensions/approval.mjs"
                    )),
                )
                .unwrap();
                if scenario == "mutation" {
                    std::fs::write(
                        profile.join("extensions/rewrite.js"),
                        include_bytes!(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/testdata/pi/extensions/rewrite.mjs"
                        )),
                    )
                    .unwrap();
                }
                let mut child = Command::new(&launch.node_path)
                    .arg(&launch.entrypoint_path)
                    .args(crate::acp::bindings::pi::native_admission::pi_launch_args(
                        crate::acp::bindings::pi::bridge::install::installed(
                            &launch.package_version,
                        )
                        .unwrap(),
                    ))
                    .args([
                        "--no-session",
                        "--provider",
                        "sage-fixture",
                        "--model",
                        "approval",
                    ])
                    .env("PI_CODING_AGENT_DIR", profile)
                    .env("SAGE_PI_FIXTURE_SCENARIO", scenario)
                    .current_dir(&j.root)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .unwrap();
                let output = child.stdout.take().unwrap();
                let (send, frames) = sync_channel(8);
                std::thread::spawn(move || {
                    let mut output = BufReader::new(output);
                    loop {
                        let mut line = Vec::new();
                        let count = output
                            .by_ref()
                            .take(1_048_577)
                            .read_until(b'\n', &mut line)
                            .unwrap();
                        if count == 0 {
                            break;
                        }
                        assert!(count <= 1_048_576, "bounded Pi RPC frame");
                        if send.send(serde_json::from_slice(&line).unwrap()).is_err() {
                            break;
                        }
                    }
                });
                let mut pi = Pi { child, frames };
                pi.send(json!({"type":"get_commands","id":"commands"}));
                let commands = (0..128)
                    .map(|_| pi.next())
                    .find(|value| value["id"] == "commands")
                    .expect("bounded command discovery");
                assert!(
                    commands["data"]["commands"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|command| command["name"] == "fixture-command")
                );
                if scenario == "mutation" {
                    assert!(
                        commands["data"]["commands"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|command| command["name"] == "fixture-rewriter")
                    );
                }
                let prompts: &[&str] = match scenario {
                    "cancel" => &["hello", "after-cancel"],
                    "collisions" => &["read", "write", "edit", "bash"],
                    _ => &["hello"],
                };
                for &prompt_text in prompts {
                    let prompt = j
                        .runtime
                        .command(AdapterCommand::Prompt {
                            prompt: Prompt::new("prompt", prompt_text).unwrap(),
                            client_message_id: RequiredText::new(
                                "client",
                                format!("native-{scenario}-{prompt_text}"),
                            )
                            .unwrap(),
                            access_mode: mode,
                        })
                        .unwrap();
                    pi.send(deliver(&mut j.runtime, prompt));
                    let mut cards = 0;
                    let mut results = Vec::new();
                    let mut cancelled = false;
                    let mut settled_seen = false;
                    for _ in 0..512 {
                        let value = pi.next();
                        if value["type"] == "tool_execution_end" {
                            results.push((
                                value["toolName"].as_str().unwrap().to_owned(),
                                value["isError"] == true,
                                value["result"]["content"][0]["text"]
                                    .as_str()
                                    .unwrap()
                                    .to_owned(),
                            ));
                        }
                        let settled = value["type"] == "agent_settled";
                        let batch = j
                            .runtime
                            .ingest(&frame(value), &FileMapper(j.root.clone()))
                            .unwrap();
                        let (events, egress) = batch.into_parts();
                        for reply in egress {
                            pi.send(deliver(&mut j.runtime, reply));
                        }
                        for event in events {
                            if let AdapterEvent::PermissionRequest {
                                external_request_id,
                                payload,
                                ..
                            } = event
                            {
                                cards += 1;
                                let NormalizedPayload::PermissionRequest {
                                    category, summary, ..
                                } = payload
                                else {
                                    panic!("approval card expected");
                                };
                                assert_eq!(
                                    category,
                                    if scenario == "concurrent"
                                        && summary.as_str() == "Allow Pi to update nested.txt?"
                                    {
                                        ToolCategory::Edit
                                    } else {
                                        ToolCategory::Extension
                                    }
                                );
                                assert!(!summary.as_str().contains("rewritten"));
                                if scenario == "mutation" {
                                    assert!(
                                        summary.as_str().contains("hello")
                                            && summary.as_str().contains("approved")
                                    );
                                }
                                if scenario == "cancel"
                                    && prompt_text == "hello"
                                    && summary.as_str().contains("fixture_echo")
                                {
                                    assert!(
                                        !cancelled,
                                        "queued approvals must cancel without opening another card"
                                    );
                                    cancelled = true;
                                    let abort = j.runtime.command(AdapterCommand::Cancel).unwrap();
                                    pi.send(deliver(&mut j.runtime, abort));
                                    continue;
                                }
                                let reply = j
                                    .runtime
                                    .command(AdapterCommand::ResolvePermission {
                                        request_id: SageServerRequestId::parse(
                                            external_request_id.as_str(),
                                        )
                                        .unwrap(),
                                        decision: decision.clone(),
                                    })
                                    .unwrap();
                                if decision == PermissionDecision::Deny {
                                    j.runtime.commit_mandatory_egress(reply.token()).unwrap();
                                }
                                pi.send(deliver(&mut j.runtime, reply));
                            }
                        }
                        if settled {
                            settled_seen = true;
                            break;
                        }
                    }
                    assert!(settled_seen, "Pi must finish the turn");
                    let denied = decision == PermissionDecision::Deny;
                    if scenario == "cancel" && prompt_text == "hello" {
                        assert!(cancelled);
                        assert_eq!(cards, 2);
                        assert!(
                            results
                                .iter()
                                .all(|(_, _, text)| !text.starts_with("Executed:"))
                        );
                    } else if ["concurrent", "overflow"].contains(&scenario) {
                        let nested = results
                            .iter()
                            .filter(|(name, _, _)| name == "fixture_echo")
                            .collect::<Vec<_>>();
                        let count = if scenario == "overflow" { 10 } else { 2 };
                        assert_eq!(nested.len(), count);
                        assert_eq!(cards, if count == 10 { 10 } else { 4 });
                        for index in 0..if count == 10 { 9 } else { 2 } {
                            assert!(nested.iter().any(|(_, error, text)| !error
                                && text == &format!("Executed: nested-{index}")));
                        }
                        if scenario == "concurrent" {
                            assert_eq!(
                                std::fs::read_to_string(j.root.join("nested.txt")).unwrap(),
                                "approved write"
                            );
                            assert!(results.iter().any(|(name, error, text)| name == "write"
                                && !error
                                && text == "Workspace file updated by Sage."));
                        }
                        assert_eq!(
                            nested.iter().filter(|(_, error, _)| !error).count(),
                            if count == 10 { 9 } else { 2 }
                        );
                        if count == 10 {
                            assert!(nested.iter().any(|(_, error, text)| *error
                                && text.contains("Sage approval queue is full")));
                        }
                        assert!(
                            results
                                .iter()
                                .any(|(name, error, _)| name == "fixture_parallel" && !error)
                        );
                    } else {
                        assert_eq!(cards, 1, "{scenario} {mode:?} {decision:?} {prompt_text}");
                        assert_eq!(results.len(), 1);
                        for (name, error, text) in results {
                            assert_eq!(
                                name,
                                if scenario == "collisions" {
                                    prompt_text
                                } else {
                                    "fixture_echo"
                                }
                            );
                            assert_eq!(error, denied);
                            assert_eq!(
                                text,
                                if denied {
                                    Refusal::Denied.message().to_owned()
                                } else if scenario == "mutation" {
                                    "Executed: hello -> approved".into()
                                } else {
                                    format!("Executed: {prompt_text}")
                                }
                            );
                        }
                    }
                }
            }
        }
    }
}
