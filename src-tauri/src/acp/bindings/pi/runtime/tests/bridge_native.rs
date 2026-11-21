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
    for mode in [
        SessionAccessMode::ReadOnly,
        SessionAccessMode::WorkspaceWrite,
    ] {
        for decision in [PermissionDecision::Deny, PermissionDecision::AllowOnce] {
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
            let mut child = Command::new(&launch.node_path)
                .arg(&launch.entrypoint_path)
                .args(crate::acp::bindings::pi::native_admission::pi_launch_args(
                    crate::acp::bindings::pi::bridge::install::installed(&launch.package_version)
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
            let prompt = j
                .runtime
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "hello").unwrap(),
                    client_message_id: RequiredText::new("client", "native-extension").unwrap(),
                    access_mode: mode,
                })
                .unwrap();
            pi.send(deliver(&mut j.runtime, prompt));
            let mut cards = 0;
            let mut result = None;
            let mut settled_seen = false;
            for _ in 0..128 {
                let value = pi.next();
                if value["type"] == "tool_execution_end" {
                    result = Some((
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
                        assert!(matches!(payload, NormalizedPayload::PermissionRequest {
                            category: ToolCategory::Extension, ref summary, ..
                        } if summary.as_str().contains("fixture_echo") && summary.as_str().contains("hello")));
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
            assert_eq!(cards, 1, "{mode:?} {decision:?}");
            assert_eq!(
                result,
                Some(if decision == PermissionDecision::Deny {
                    (true, Refusal::Denied.message().to_owned())
                } else {
                    (false, "Executed: hello".into())
                })
            );
        }
    }
}
