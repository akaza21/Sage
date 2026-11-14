use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::codex::manifest::parse_packet_metadata, contract::RuntimeEpoch,
    transport::IncrementalFrameDecoder,
};

struct ScriptTransport {
    reads: VecDeque<Result<Option<Frame>, CodexDiscoveryRunError>>,
    writes: Vec<Value>,
    drift_at: Option<RevalidationCheckpoint>,
    write_failure_at: Option<usize>,
    closed: bool,
    terminated: Option<bool>,
    cleanup: Result<u64, CodexDiscoveryRunError>,
    read_timeouts: Vec<Duration>,
    cleanup_end: Option<Instant>,
    now: Instant,
}

impl ScriptTransport {
    fn happy() -> Self {
        Self {
            reads: VecDeque::from([
                Ok(Some(frame(json!({"id":1,"result":initialize_result()})))),
                Ok(Some(frame(json!({"id":2,"result":{
                    "account":{"type":"chatgpt","email":"secret@example.test","planType":"plus","workspaceName":null},
                    "requiresOpenaiAuth":false,"workspaceRouting":null
                }})))),
                Ok(Some(frame(json!({"id":3,"result":{
                    "data":[model("gpt-a", "GPT A", true)],"nextCursor":null
                }})))),
            ]),
            writes: Vec::new(),
            drift_at: None,
            write_failure_at: None,
            closed: false,
            terminated: None,
            cleanup: Ok(71),
            read_timeouts: Vec::new(),
            cleanup_end: None,
            now: Instant::now(),
        }
    }
}

impl DiscoveryTransport for ScriptTransport {
    fn now(&self) -> Instant {
        self.now
    }

    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.writes
            .push(serde_json::from_slice(frame.payload()).unwrap());
        if self.write_failure_at == Some(self.writes.len()) {
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 }
        } else {
            FrameWriteOutcome::Complete
        }
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        self.read_timeouts.push(timeout);
        self.reads.pop_front().unwrap_or_else(|| {
            self.now += timeout;
            Err(CodexDiscoveryRunError::Timeout)
        })
    }

    fn revalidate(&self, checkpoint: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
        if self.drift_at == Some(checkpoint) {
            Err(CodexDiscoveryRunError::Drift)
        } else {
            Ok(())
        }
    }

    fn close_stdin(&mut self) {
        self.closed = true;
    }

    fn terminate_zero(
        &mut self,
        protocol_failure: bool,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        self.terminated = Some(protocol_failure);
        self.cleanup_end = Some(deadline.cleanup_end());
        self.cleanup.map(DiscoveryZeroProcess::fixture)
    }
}

#[test]
fn success_is_returned_only_after_stdin_close_and_matching_zero_process_proof() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();

    let deadline = deadline();
    let content = run_discovery_protocol(&mut protocol, &mut transport, 71, deadline).unwrap();

    assert_eq!(content.models[0].id.as_str(), "gpt-a");
    assert!(transport.closed);
    assert_eq!(transport.terminated, Some(false));
    assert_eq!(transport.writes.len(), 4);
    assert_eq!(transport.cleanup_end, Some(deadline.cleanup_end()));
}

#[test]
fn pinned_codex_notification_between_initialize_and_account_response_is_ignored() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.reads.insert(
        1,
        Ok(Some(frame(json!({
            "method": "remoteControl/status/changed",
            "emittedAtMs": 0,
            "params": {"environmentId": null, "status": "disabled"}
        })))),
    );

    let content = run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()).unwrap();

    assert_eq!(content.models[0].id.as_str(), "gpt-a");
    assert_eq!(transport.terminated, Some(false));
}

#[test]
fn recognized_codex_notifications_are_skipped_before_the_model_response() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.reads.insert(
        2,
        Ok(Some(frame(json!({
            "method": "account/updated",
            "params": {"authMode": "chatgpt", "planType": "plus"}
        })))),
    );
    transport.reads.insert(
        3,
        Ok(Some(frame(json!({
            "method": "remoteControl/status/changed",
            "emittedAtMs": 0,
            "params": {"environmentId": null, "status": "disabled"}
        })))),
    );

    let content = run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()).unwrap();

    assert_eq!(content.models[0].id.as_str(), "gpt-a");
    assert_eq!(transport.terminated, Some(false));
}

#[test]
fn pinned_codex_model_response_before_account_response_is_correlated_by_id() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.reads.swap(1, 2);

    let content = run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()).unwrap();

    assert_eq!(content.models[0].id.as_str(), "gpt-a");
    assert_eq!(transport.terminated, Some(false));
}

#[test]
fn a_config_warning_and_unknown_notifications_during_discovery_are_skipped() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.reads.insert(
        1,
        Ok(Some(frame(json!({
            "method": "configWarning",
            "params": {"summary": "unknown key", "details": null}
        })))),
    );
    transport.reads.insert(
        2,
        Ok(Some(frame(json!({"method": "future/event", "params": {}})))),
    );
    transport
        .reads
        .insert(3, Ok(Some(frame(json!({"method": "future/bare"})))));

    let content = run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()).unwrap();

    assert_eq!(content.models[0].id.as_str(), "gpt-a");
    assert_eq!(transport.terminated, Some(false));
}

#[test]
fn a_server_request_during_discovery_fails_closed() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.reads.insert(
        1,
        Ok(Some(frame(json!({
            "id": 900,
            "method": "item/commandExecution/requestApproval",
            "params": {}
        })))),
    );

    assert_eq!(
        run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()),
        Err(CodexDiscoveryRunError::Protocol)
    );
    assert!(transport.closed);
    assert_eq!(transport.terminated, Some(true));
}

#[test]
fn sixteen_notifications_before_a_response_fails_closed() {
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    for index in 0..16 {
        transport.reads.insert(
            0,
            Ok(Some(frame(json!({
                "method": "remoteControl/status/changed",
                "emittedAtMs": index,
                "params": {"environmentId": null, "status": "disabled"}
            })))),
        );
    }

    assert_eq!(
        run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()),
        Err(CodexDiscoveryRunError::Protocol)
    );
    assert!(transport.closed);
    assert_eq!(transport.terminated, Some(true));
}

#[test]
fn every_transport_protocol_eof_timeout_and_drift_failure_tears_down() {
    let cases = [
        CodexDiscoveryRunError::Eof,
        CodexDiscoveryRunError::Timeout,
        CodexDiscoveryRunError::Transport,
    ];
    for expected in cases {
        let mut protocol = protocol();
        let mut transport = ScriptTransport::happy();
        transport.reads[0] = match expected {
            CodexDiscoveryRunError::Eof => Ok(None),
            error => Err(error),
        };

        assert_eq!(
            run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()),
            Err(expected)
        );
        assert!(transport.closed);
        assert_eq!(transport.terminated, Some(true));
    }

    let mut protocol = protocol();
    let mut malformed = ScriptTransport::happy();
    malformed.reads[0] = Ok(Some(frame(
        json!({"method":"future/event","params":"text"}),
    )));
    assert_eq!(
        run_discovery_protocol(&mut protocol, &mut malformed, 71, deadline()),
        Err(CodexDiscoveryRunError::Protocol)
    );
    assert!(malformed.closed);
    assert_eq!(malformed.terminated, Some(true));
}

#[test]
fn a_model_list_that_never_answers_expires_its_read_budget_and_tears_down() {
    let admitted = Instant::now();
    let budget = Duration::from_millis(20);
    let deadline =
        CodexDiscoveryDeadline::from_budgets(admitted, budget, Duration::from_millis(20)).unwrap();
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();
    transport.now = admitted;
    transport.reads.pop_back();

    assert_eq!(
        run_discovery_protocol(&mut protocol, &mut transport, 71, deadline),
        Err(CodexDiscoveryRunError::Timeout)
    );
    assert_eq!(transport.read_timeouts, vec![budget; 3]);
    assert_eq!(transport.now, deadline.operation_end());
    assert_eq!(transport.writes.len(), 4);
    assert_eq!(transport.writes[3]["method"], "model/list");
    assert_eq!(transport.cleanup_end, Some(deadline.cleanup_end()));
    assert!(transport.closed);
    assert_eq!(transport.terminated, Some(true));
}

#[test]
fn cleanup_failure_or_wrong_epoch_never_publishes_a_completed_snapshot() {
    for cleanup in [Err(CodexDiscoveryRunError::Cleanup), Ok(72)] {
        let mut protocol = protocol();
        let mut transport = ScriptTransport::happy();
        transport.cleanup = cleanup;
        assert_eq!(
            run_discovery_protocol(&mut protocol, &mut transport, 71, deadline()),
            Err(CodexDiscoveryRunError::Cleanup)
        );
        assert!(transport.closed);
    }
}

#[test]
fn one_absolute_deadline_shrinks_across_phase_caps() {
    let admitted = Instant::now();
    let deadline = CodexDiscoveryDeadline::from_budgets(
        admitted,
        Duration::from_millis(90),
        Duration::from_millis(10),
    )
    .unwrap();

    assert_eq!(
        deadline
            .operation_remaining_at(admitted, Duration::from_millis(30))
            .unwrap(),
        Duration::from_millis(30)
    );
    assert_eq!(
        deadline
            .operation_remaining_at(
                admitted + Duration::from_millis(80),
                Duration::from_millis(30),
            )
            .unwrap(),
        Duration::from_millis(10)
    );
    assert_eq!(
        deadline.operation_remaining_at(
            admitted + Duration::from_millis(90),
            Duration::from_millis(30),
        ),
        Err(CodexDiscoveryRunError::Timeout)
    );
    assert_eq!(
        deadline.cleanup_end(),
        admitted + Duration::from_millis(100)
    );
}

#[test]
fn cumulative_expiry_still_closes_and_requires_zero_process_proof() {
    let admitted = Instant::now()
        .checked_sub(Duration::from_millis(2))
        .unwrap();
    let deadline = CodexDiscoveryDeadline::from_budgets(
        admitted,
        Duration::from_millis(1),
        Duration::from_secs(1),
    )
    .unwrap();
    let mut protocol = protocol();
    let mut transport = ScriptTransport::happy();

    assert_eq!(
        run_discovery_protocol(&mut protocol, &mut transport, 71, deadline),
        Err(CodexDiscoveryRunError::Timeout)
    );
    assert!(transport.closed);
    assert_eq!(transport.terminated, Some(true));
    assert!(transport.writes.is_empty());
}

fn deadline() -> CodexDiscoveryDeadline {
    CodexDiscoveryDeadline::from_budgets(
        Instant::now(),
        Duration::from_secs(1),
        Duration::from_secs(1),
    )
    .unwrap()
}

fn protocol() -> CodexDiscoveryProtocol {
    let packet = parse_packet_metadata(include_bytes!(
        "../../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    CodexDiscoveryProtocol::new(&packet, [9; 32], RuntimeEpoch::new(71).unwrap())
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    decoder.push_chunk(&bytes).unwrap().unwrap()
}

fn initialize_result() -> Value {
    json!({
        "userAgent":user_agent("sage", "terminal"),
        "codexHome":MANAGED_CODEX_HOME,
        "platformFamily":"windows",
        "platformOs":"windows"
    })
}

fn model(id: &str, name: &str, is_default: bool) -> Value {
    json!({
        "id":id,"model":id,"upgrade":null,"upgradeInfo":null,
        "availabilityNux":null,"displayName":name,"description":"Pinned model",
        "hidden":false,"supportedReasoningEfforts":[{
            "reasoningEffort":"medium","description":"Medium"
        }],
        "defaultReasoningEffort":"medium","inputModalities":["text"],
        "supportsPersonality":false,"additionalSpeedTiers":[],"serviceTiers":[],
        "defaultServiceTier":null,"modelSpecialty":null,"multiAgentVersion":null,"availableAccessPrograms":null,"isDefault":is_default
    })
}
