//! The generated `pi-bridge-*` fixture cases replayed against the real bridge
//! runtime: the child's dialogs reach Sage's permission request and write path,
//! and the child observes Sage's answers exactly as the extension would.
use super::*;
use std::{cell::RefCell, collections::VecDeque, io, rc::Rc};

type Replies = Rc<RefCell<VecDeque<u8>>>;

/// Sage's side of the child's stdio: ingests each frame, answers the approval
/// card with a fixed decision and queues every reply for the child to read.
struct Sage<'a> {
    journey: &'a mut Journey,
    replies: Replies,
    decision: PermissionDecision,
    line: Vec<u8>,
    cards: usize,
    tool_errors: Vec<bool>,
}

fn queue(replies: &Replies, runtime: &mut PiRuntime, egress: AdapterEgress) {
    let mut bytes = serde_json::to_vec(&deliver(runtime, egress)).unwrap();
    bytes.push(b'\n');
    replies.borrow_mut().extend(bytes);
}

impl Sage<'_> {
    fn ingest(&mut self, value: Value) {
        if value["type"] == "tool_execution_end" {
            self.tool_errors.push(value["isError"] == true);
        }
        let mapper = FileMapper(self.journey.root.clone());
        let runtime = &mut self.journey.runtime;
        let (events, egress) = runtime
            .ingest(&frame(value.clone()), &mapper)
            .unwrap()
            .into_parts();
        for reply in egress {
            queue(&self.replies, runtime, reply);
        }
        if events
            .iter()
            .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        {
            self.cards += 1;
            let answer = runtime
                .command(AdapterCommand::ResolvePermission {
                    request_id: SageServerRequestId::parse(value["id"].as_str().unwrap()).unwrap(),
                    decision: self.decision,
                })
                .unwrap();
            if self.decision == PermissionDecision::Deny {
                runtime.commit_mandatory_egress(answer.token()).unwrap();
            }
            queue(&self.replies, runtime, answer);
        }
    }
}

impl io::Write for Sage<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        for &byte in bytes {
            if byte == b'\n' {
                let line = std::mem::take(&mut self.line);
                self.ingest(serde_json::from_slice(&line).unwrap());
            } else {
                self.line.push(byte);
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Child(Replies);

impl io::Read for Child {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let mut replies = self.0.borrow_mut();
        let count = buffer.len().min(replies.len());
        for (slot, byte) in buffer.iter_mut().zip(replies.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

struct Outcome {
    journey: Journey,
    cards: usize,
    tool_errors: Vec<bool>,
}

fn replay(case: &str, decision: PermissionDecision) -> Outcome {
    let mut journey = Journey::new(SessionAccessMode::WorkspaceWrite);
    std::fs::create_dir(journey.root.join("src")).unwrap();
    std::fs::write(
        journey.root.join("src").join("example.ts"),
        "export const example = 1;\n",
    )
    .unwrap();
    let replies = Replies::default();
    let mut sage = Sage {
        journey: &mut journey,
        replies: Rc::clone(&replies),
        decision,
        line: Vec::new(),
        cards: 0,
        tool_errors: Vec::new(),
    };
    for call in crate::acp::fixture::pi_bridge_calls_for_test(case) {
        crate::acp::fixture::run_pi_bridge_call_for_test(
            &call,
            &mut Child(Rc::clone(&replies)),
            &mut sage,
        );
    }
    let (cards, tool_errors) = (sage.cards, sage.tool_errors);
    assert!(replies.borrow().is_empty(), "{case}: unread Sage reply");
    Outcome {
        journey,
        cards,
        tool_errors,
    }
}

fn text(outcome: &Outcome, path: &str) -> Option<String> {
    std::fs::read_to_string(outcome.journey.root.join(path)).ok()
}

#[test]
fn pi_bridge_fixture_cases_drive_the_real_approval_and_write_path() {
    use PermissionDecision::{AllowOnce, Deny};
    let created = "src/bridge-note.txt";
    let approved = replay("pi-bridge-write-new", AllowOnce);
    assert_eq!(
        (approved.cards, approved.tool_errors.as_slice()),
        (1, &[false][..])
    );
    assert_eq!(
        text(&approved, created).as_deref(),
        Some("Created through the Sage bridge.\n")
    );
    let denied = replay("pi-bridge-write-new", Deny);
    assert_eq!(
        (denied.cards, denied.tool_errors.as_slice()),
        (1, &[true][..])
    );
    assert_eq!(text(&denied, created), None);

    let edited = replay("pi-bridge-edit-existing", AllowOnce);
    assert_eq!(
        (edited.cards, edited.tool_errors.as_slice()),
        (1, &[false][..])
    );
    assert_eq!(
        text(&edited, "src/example.ts").as_deref(),
        Some("export const example = 2;\n")
    );
    let kept = replay("pi-bridge-edit-existing", Deny);
    assert_eq!((kept.cards, kept.tool_errors.as_slice()), (1, &[true][..]));
    assert_eq!(
        text(&kept, "src/example.ts").as_deref(),
        Some("export const example = 1;\n")
    );

    // Refused before any card: nothing for the user to approve.
    for case in [
        "pi-bridge-git-config-refused",
        "pi-bridge-device-name-refused",
    ] {
        let refused = replay(case, AllowOnce);
        assert_eq!(
            (refused.cards, refused.tool_errors.as_slice()),
            (0, &[true][..]),
            "{case}"
        );
        assert!(!refused.journey.root.join(".git").exists(), "{case}");
        assert_eq!(refused.journey.runtime.bridge.as_ref().unwrap().writes, 0);
    }

    // The replayed prepare for an already approved call gets no second card.
    let replayed = replay("pi-bridge-replay-refused", AllowOnce);
    assert_eq!(
        (replayed.cards, replayed.tool_errors.as_slice()),
        (1, &[false][..])
    );
    assert_eq!(
        text(&replayed, "src/bridge-replay.txt").as_deref(),
        Some("First approved write.\n")
    );
}

#[cfg(all(windows, debug_assertions))]
#[test]
fn pi_fixture_admits_the_bridge_only_for_cases_that_replay_it() {
    for case in [
        "pi-bridge-write-new",
        "pi-bridge-edit-existing",
        "pi-bridge-git-config-refused",
        "pi-bridge-device-name-refused",
        "pi-bridge-replay-refused",
    ] {
        assert!(
            crate::acp::fixture::native_case_uses_pi_bridge(case),
            "{case}"
        );
    }
    // Without the extension, Pi's write and shell tools stay authority-blocked.
    for case in [
        "pi-non-read-denial",
        "pi-turn-completed",
        "pi-bridge-unknown",
    ] {
        assert!(
            !crate::acp::fixture::native_case_uses_pi_bridge(case),
            "{case}"
        );
    }
}
