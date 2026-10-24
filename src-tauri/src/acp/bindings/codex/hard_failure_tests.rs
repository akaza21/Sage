use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tracing_subscriber::fmt::MakeWriter;

use crate::acp::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{NormalizedPayload, Prompt, SafeResourceRef, UnavailableResourceReason},
    lifecycle::ServerRequestState,
};

use super::incoming::{ItemUpdateMode, UNPROJECTED_DIAGNOSTIC};
use super::state::CodexAdapter;
use super::{IncomingEvent, IngressDisposition, ResourceMapper, parse_packet_metadata};

const PACKET_FIXTURE: &[u8] =
    include_bytes!("../../../../testdata/acp/codex/candidate-windows.json");

const UNIMPLEMENTED_SERVER_REQUESTS: [&str; 7] = [
    "item/tool/requestUserInput",
    "mcpServer/elicitation/request",
    "account/chatgptAuthTokens/refresh",
    "attestation/generate",
    "currentTime/read",
    "applyPatchApproval",
    "execCommandApproval",
];

const UNPROJECTED_NOTIFICATIONS: [&str; 5] = [
    "fs/changed",
    "hook/started",
    "thread/realtime/started",
    "turn/diff/updated",
    "warning",
];

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

#[derive(Clone)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

pub(super) fn with_captured_diagnostics<T>(body: impl FnOnce() -> T) -> (T, String) {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(Capture(Arc::clone(&bytes)))
        .finish();
    let value = tracing::subscriber::with_default(subscriber, body);
    let captured = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    (value, captured)
}

fn adapter() -> CodexAdapter {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    CodexAdapter::from_metadata_packet(
        &packet,
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("session-one").unwrap(),
    )
}

fn initialize_result(id: u64) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "id": id,
        "result": {
            "userAgent": "sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
            "codexHome": "C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
            "platformFamily": "windows",
            "platformOs": "windows"
        }
    }))
    .unwrap()
}

fn ready(adapter: &mut CodexAdapter) {
    adapter.initialize(900).unwrap();
    adapter.ingest(&initialize_result(900), &Mapper).unwrap();
    adapter.initialized().unwrap();
    adapter
        .start(1, &adapter.project_root(r"C:\repo".into()).unwrap(), None)
        .unwrap();
    adapter
        .ingest(
            br#"{"id":1,"result":{"thread":{"id":"thread-1"}}}"#,
            &Mapper,
        )
        .unwrap();
}

fn running_turn(adapter: &mut CodexAdapter) {
    let prompt = Prompt::new("prompt", "inspect").unwrap();
    let client = RequiredText::new("client", "client-hard-failure").unwrap();
    adapter.prompt(2, &prompt, &client).unwrap();
    let started = json!({"method":"turn/started","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
    }});
    adapter
        .ingest(&serde_json::to_vec(&started).unwrap(), &Mapper)
        .unwrap();
}

fn ingest(adapter: &mut CodexAdapter, frame: &Value) -> IngressDisposition {
    adapter
        .ingest(&serde_json::to_vec(frame).unwrap(), &Mapper)
        .unwrap()
}

#[test]
fn unimplemented_server_requests_are_answered_rather_than_tearing_the_runtime_down() {
    let mut adapter = adapter();
    ready(&mut adapter);
    for (offset, method) in UNIMPLEMENTED_SERVER_REQUESTS.iter().enumerate() {
        let id = 9000 + offset as u64;
        let frame = json!({"id": id, "method": method, "params": {}});
        let disposition = adapter
            .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
            .unwrap_or_else(|error| panic!("{method} must not tear the runtime down: {error}"));
        let IngressDisposition::Events(events) = disposition else {
            panic!("{method} must answer the caller");
        };
        let [IncomingEvent::HostToolReply { bytes }] = events.as_slice() else {
            panic!("{method} must produce exactly one wire reply");
        };
        let reply: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(reply["id"], json!(id), "{method} must correlate its reply");
        assert_eq!(reply["error"]["code"], json!(-32601), "{method}");
        assert!(reply.get("result").is_none(), "{method} did not succeed");
        assert!(
            !reply["error"]["message"].as_str().unwrap().is_empty(),
            "{method} must say why"
        );
    }
}

#[test]
fn an_unimplemented_server_request_leaves_the_session_able_to_run_a_turn() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let frame = json!({"id": 4242, "method": "currentTime/read", "params": {}});
    ingest(&mut adapter, &frame);
    running_turn(&mut adapter);
    let completed = json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &completed) else {
        panic!("the session must still complete a turn");
    };
    assert!(matches!(events[0], IncomingEvent::Turn { .. }));
}

#[test]
fn a_known_approval_on_an_unknown_turn_still_fails_hard_instead_of_replying_not_found() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let frame = json!({"id": 51, "method": "item/fileChange/requestApproval", "params": {
        "threadId": "thread-1", "turnId": "turn-absent", "itemId": "item-1"
    }});
    assert!(
        adapter
            .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
            .is_err(),
        "a known approval on a stale turn must not be laundered into a polite error reply"
    );
}

#[test]
fn malformed_thread_goal_updated_is_ignored_without_failing_the_managed_session() {
    let mut adapter = adapter();
    ready(&mut adapter);
    let frame = json!({"method":"thread/goal/updated","params":{
        "threadId":"thread-1","goal":{"text":"ship the release"}
    }});
    let (disposition, captured) =
        with_captured_diagnostics(|| adapter.ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper));
    assert!(
        matches!(disposition, Ok(IngressDisposition::IgnoredLate)),
        "thread/goal/updated must not kill the session: {disposition:?}"
    );
    assert_eq!(captured.matches(UNPROJECTED_DIAGNOSTIC).count(), 0);
}

#[test]
fn unprojected_notifications_are_recorded_and_ignored_without_transcript_items() {
    let mut adapter = adapter();
    ready(&mut adapter);
    for method in UNPROJECTED_NOTIFICATIONS {
        let frame = json!({"method": method, "params": {"threadId": "thread-1"}});
        let (disposition, captured) = with_captured_diagnostics(|| {
            adapter.ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
        });
        assert!(
            matches!(disposition, Ok(IngressDisposition::IgnoredLate)),
            "{method} must be ignored rather than projected: {disposition:?}"
        );
        assert_eq!(
            captured.matches(UNPROJECTED_DIAGNOSTIC).count(),
            1,
            "{method} must be recorded exactly once"
        );
    }
}

#[test]
fn server_request_resolved_cancels_the_pending_permission_row() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let approval = json!({"id": 61, "method": "item/fileChange/requestApproval", "params": {
        "threadId": "thread-1", "turnId": "turn-1", "itemId": "item-1"
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &approval) else {
        panic!("the approval must register a permission row");
    };
    assert!(matches!(events[0], IncomingEvent::Permission { .. }));

    let resolved = json!({"method":"serverRequest/resolved","params":{
        "threadId":"thread-1","requestId":61
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &resolved) else {
        panic!("a harness-side resolution must terminalize the row");
    };
    let [IncomingEvent::PermissionResolved { state, .. }] = events.as_slice() else {
        panic!("expected exactly one permission resolution, got {events:?}");
    };
    assert_eq!(
        *state,
        ServerRequestState::Cancelled,
        "the wire carries no decision, so only Cancelled is truthful"
    );
}

#[test]
fn a_repeated_server_request_resolved_is_ignored_rather_than_conflicting() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let approval = json!({"id": 62, "method": "item/commandExecution/requestApproval", "params": {
        "threadId": "thread-1", "turnId": "turn-1", "itemId": "item-2"
    }});
    ingest(&mut adapter, &approval);
    let resolved = json!({"method":"serverRequest/resolved","params":{
        "threadId":"thread-1","requestId":62
    }});
    ingest(&mut adapter, &resolved);
    assert!(matches!(
        ingest(&mut adapter, &resolved),
        IngressDisposition::IgnoredLate
    ));
}

#[test]
fn server_request_resolved_for_an_unknown_request_is_ignored_rather_than_fatal() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let resolved = json!({"method":"serverRequest/resolved","params":{
        "threadId":"thread-1","requestId":"never-registered"
    }});
    assert!(matches!(
        ingest(&mut adapter, &resolved),
        IngressDisposition::IgnoredLate
    ));
}

#[test]
fn reasoning_summary_deltas_coalesce_into_one_thought_and_complete() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);

    let started = json!({"method":"item/started","params":{
        "threadId":"thread-1","turnId":"turn-1",
        "item":{"type":"reasoning","id":"reason-1"}
    }});
    assert!(
        matches!(ingest(&mut adapter, &started), IngressDisposition::Events(events) if events.is_empty()),
        "an empty reasoning item must not emit a placeholder thought"
    );

    for delta in ["Checking the ", "adapter contract"] {
        let frame = json!({"method":"item/reasoning/summaryTextDelta","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"reason-1",
            "summaryIndex":0,"delta":delta
        }});
        let IngressDisposition::Events(events) = ingest(&mut adapter, &frame) else {
            panic!("a summary delta must stream");
        };
        let [
            IncomingEvent::Item {
                payload: NormalizedPayload::ThoughtSummary { summary },
                update_mode,
                ..
            },
        ] = events.as_slice()
        else {
            panic!("a summary delta must append a thought, got {events:?}");
        };
        assert_eq!(summary.as_str(), delta);
        assert_eq!(*update_mode, ItemUpdateMode::Append);
    }

    let completed = json!({"method":"item/completed","params":{
        "threadId":"thread-1","turnId":"turn-1",
        "item":{"type":"reasoning","id":"reason-1",
                "summary":["Checking the adapter contract"]}
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &completed) else {
        panic!("the completed reasoning item must project its real text");
    };
    let [
        IncomingEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { summary },
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected a completed thought, got {events:?}");
    };
    assert_eq!(
        summary.as_str(),
        "Checking the adapter contract",
        "the completed item must carry the real reasoning text, never a placeholder"
    );
}

#[test]
fn raw_reasoning_content_deltas_are_accepted_but_never_projected() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let frame = json!({"method":"item/reasoning/textDelta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"reason-2",
        "contentIndex":0,"delta":"weighing the options"
    }});
    assert!(
        matches!(
            ingest(&mut adapter, &frame),
            IngressDisposition::DroppedRawReasoning
        ),
        "Codex shows the reasoning summary only; the drop is deliberate, not late"
    );
}

#[test]
fn a_new_reasoning_summary_part_separates_coalesced_thoughts() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let first = json!({"method":"item/reasoning/summaryPartAdded","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"reason-3","summaryIndex":0
    }});
    assert!(
        matches!(
            ingest(&mut adapter, &first),
            IngressDisposition::IgnoredLate
        ),
        "the first summary part needs no separator"
    );
    let second = json!({"method":"item/reasoning/summaryPartAdded","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"reason-3","summaryIndex":1
    }});
    let IngressDisposition::Events(events) = ingest(&mut adapter, &second) else {
        panic!("a later summary part must separate itself from the previous one");
    };
    let [
        IncomingEvent::Item {
            payload: NormalizedPayload::ThoughtSummary { summary },
            update_mode,
            ..
        },
    ] = events.as_slice()
    else {
        panic!("expected a separator append, got {events:?}");
    };
    assert_eq!(summary.as_str(), "\n\n");
    assert_eq!(*update_mode, ItemUpdateMode::Append);
}

#[test]
fn an_unknown_dynamic_tool_call_is_refused_without_tearing_the_runtime_down() {
    let mut adapter = adapter();
    ready(&mut adapter);
    running_turn(&mut adapter);
    let request = json!({"id":"unknown-tool-1","method":"item/tool/call","params":{
        "threadId":"thread-1","turnId":"turn-1","callId":"call-1",
        "tool":"mystery_tool","arguments":{"url":"http://127.0.0.1:43124/proof"}
    }});
    let (disposition, diagnostics) = with_captured_diagnostics(|| {
        adapter.ingest(&serde_json::to_vec(&request).unwrap(), &Mapper)
    });
    let IngressDisposition::Events(events) = disposition
        .unwrap_or_else(|error| panic!("an unknown tool must not tear the runtime down: {error}"))
    else {
        panic!("an unknown tool call must answer the caller");
    };
    let [IncomingEvent::HostToolReply { bytes }] = events.as_slice() else {
        panic!("expected exactly one wire reply, got {events:?}");
    };
    let reply: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(reply["id"], json!("unknown-tool-1"));
    assert_eq!(reply["result"]["success"], json!(false));
    assert_eq!(reply.get("error"), None);
    assert!(
        diagnostics.contains(UNPROJECTED_DIAGNOSTIC),
        "the refusal must be recorded, got {diagnostics}"
    );

    let running = json!({"method":"item/agentMessage/textDelta","params":{
        "threadId":"thread-1","turnId":"turn-1","itemId":"message-1","delta":"still alive"
    }});
    adapter
        .ingest(&serde_json::to_vec(&running).unwrap(), &Mapper)
        .expect("the turn must survive an unknown tool call");
}
