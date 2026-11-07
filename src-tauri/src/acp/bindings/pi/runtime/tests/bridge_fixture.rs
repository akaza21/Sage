//! The generated `pi-bridge-*` fixture cases replayed against the real bridge
//! runtime: the child's dialogs reach Sage's permission request and write path,
//! each decision passes the host permission service exactly as the approval
//! card's does, and the child observes Sage's answers as the extension would.
use super::*;
use crate::acp::service::{
    AcpService, PermissionDecisionIntent, admit_permission_request, service_with_pending_requests,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io,
    rc::Rc,
    sync::{Arc, Mutex},
};

type Replies = Rc<RefCell<VecDeque<u8>>>;

/// The host side of the approval card: the permission service admits the
/// card's decision against the request Sage recorded, then forwards it.
struct Card {
    service: AcpService,
    owner: crate::acp::scheduler::RuntimeOwner,
    forwarded: Arc<Mutex<Vec<AdapterCommand>>>,
}

impl Card {
    fn new() -> Self {
        let forwarded = Arc::new(Mutex::new(Vec::new()));
        let (service, owner) = service_with_pending_requests(Arc::clone(&forwarded));
        Self {
            service,
            owner,
            forwarded,
        }
    }

    /// Records the request as the store would and answers it through the
    /// service; returns the command the service forwards to the runtime.
    fn decide(
        &self,
        request_id: &str,
        payload: &NormalizedPayload,
        decision: PermissionDecision,
    ) -> AdapterCommand {
        admit_permission_request(&self.service, &self.owner, request_id, payload);
        self.service
            .resolve_permission_decision(PermissionDecisionIntent {
                project_id: self.owner.project_id.clone(),
                chat_id: self.owner.chat_id.clone(),
                session_id: self.owner.session_id.clone(),
                turn_id: crate::acp::contract::SageTurnId::parse("turn-permission").unwrap(),
                runtime_epoch: self.owner.runtime_epoch,
                request_id: SageServerRequestId::parse(request_id).unwrap(),
                decision,
            })
            .unwrap_or_else(|error| panic!("the card cannot answer {decision:?}: {error:?}"));
        self.forwarded
            .lock()
            .unwrap()
            .pop()
            .expect("forwarded decision")
    }
}

impl Drop for Card {
    fn drop(&mut self) {
        let _ = self.service.shutdown_blocking();
    }
}

/// Sage's side of the child's stdio: ingests each frame, answers the approval
/// card with a fixed decision and queues every reply for the child to read.
struct Sage<'a> {
    journey: &'a mut Journey,
    card: Card,
    replies: Replies,
    decision: PermissionDecision,
    line: Vec<u8>,
    cards: usize,
    tool_errors: Vec<(bool, String)>,
}

fn queue(replies: &Replies, runtime: &mut PiRuntime, egress: AdapterEgress) {
    let mut bytes = serde_json::to_vec(&deliver(runtime, egress)).unwrap();
    bytes.push(b'\n');
    replies.borrow_mut().extend(bytes);
}

impl Sage<'_> {
    fn ingest(&mut self, value: Value) {
        if value["type"] == "tool_execution_end" {
            self.tool_errors.push((
                value["isError"] == true,
                value["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            ));
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
        let requested = events.iter().find_map(|event| match event {
            AdapterEvent::PermissionRequest { payload, .. } => Some(payload),
            _ => None,
        });
        if let Some(payload) = requested {
            self.cards += 1;
            let command = self
                .card
                .decide(value["id"].as_str().unwrap(), payload, self.decision);
            let answer = runtime.command(command).unwrap();
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
    /// Each finished tool call's error flag and result text, as Pi reports it.
    tool_errors: Vec<(bool, String)>,
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
        card: Card::new(),
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
    let (cards, tool_errors) = (sage.cards, std::mem::take(&mut sage.tool_errors));
    drop(sage);
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

fn ended(outcome: &Outcome) -> Vec<(bool, &str)> {
    outcome
        .tool_errors
        .iter()
        .map(|(error, text)| (*error, text.as_str()))
        .collect()
}

const WRITTEN: (bool, &str) = (false, "Workspace file updated by Sage.");

#[test]
fn pi_bridge_fixture_cases_drive_the_real_approval_and_write_path() {
    use PermissionDecision::{AllowOnce, Deny};
    let denied_text = (true, Refusal::Denied.message());
    let created = "src/bridge-note.txt";
    let approved = replay("pi-bridge-write-new", AllowOnce);
    assert_eq!((approved.cards, ended(&approved)), (1, vec![WRITTEN]));
    assert_eq!(
        text(&approved, created).as_deref(),
        Some("Created through the Sage bridge.\n")
    );
    let denied = replay("pi-bridge-write-new", Deny);
    assert_eq!((denied.cards, ended(&denied)), (1, vec![denied_text]));
    assert_eq!(text(&denied, created), None);

    let edited = replay("pi-bridge-edit-existing", AllowOnce);
    assert_eq!((edited.cards, ended(&edited)), (1, vec![WRITTEN]));
    assert_eq!(
        text(&edited, "src/example.ts").as_deref(),
        Some("export const example = 2;\n")
    );
    let kept = replay("pi-bridge-edit-existing", Deny);
    assert_eq!((kept.cards, ended(&kept)), (1, vec![denied_text]));
    assert_eq!(
        text(&kept, "src/example.ts").as_deref(),
        Some("export const example = 1;\n")
    );

    // Refused before any card, with Sage's reason as the tool's error.
    for case in [
        "pi-bridge-git-config-refused",
        "pi-bridge-device-name-refused",
    ] {
        let refused = replay(case, AllowOnce);
        assert_eq!(
            (refused.cards, ended(&refused)),
            (0, vec![(true, Refusal::ProtectedPath.message())]),
            "{case}"
        );
        assert!(!refused.journey.root.join(".git").exists(), "{case}");
        assert_eq!(refused.journey.runtime.bridge.as_ref().unwrap().writes, 0);
    }

    // The replayed prepare gets no second card whatever the first decision was;
    // the fixture itself fails unless Sage refuses the replay.
    let replayed = replay("pi-bridge-replay-refused", AllowOnce);
    assert_eq!((replayed.cards, ended(&replayed)), (1, vec![WRITTEN]));
    assert_eq!(
        text(&replayed, "src/bridge-replay.txt").as_deref(),
        Some("First approved write.\n")
    );
    let replayed = replay("pi-bridge-replay-refused", Deny);
    assert_eq!((replayed.cards, ended(&replayed)), (1, vec![denied_text]));
    assert_eq!(text(&replayed, "src/bridge-replay.txt"), None);
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
