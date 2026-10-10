use std::{collections::VecDeque, future::Future, pin::Pin, sync::Mutex};

use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterIdentity, EgressToken},
    contract::{EntityRevision, SafeText, SchemaVersionV1},
    intent::{
        ActivityProvenanceV1, ActivitySourceKind, CapabilitySnapshotV1, Prompt,
        SafeResourceLocation, SafeResourceRef, ToolCategory, UnavailableResourceReason,
    },
    lifecycle::{ActivityLifecycle, DeliveryState, ServerRequestState},
    projection::ActivityRole,
    runtime::ProjectionPublishError,
};

use super::coalescer::{NormalizedFact, PendingIngress};
use super::*;
use crate::acp::bindings::adapter::RefusalDetail;
use crate::acp::bindings::codex::{CodexRuntime, Platform};

#[test]
fn goal_active_response_first_mismatch_never_commits_the_provisional_turn() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
        })), 1).await.unwrap());
        assert!(shared.0.lock().unwrap().durable_batches.is_empty(),
            "response Turn A is provisional, not durable");
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-B"}}
        })), 2).await.unwrap());
        assert!(shared.0.lock().unwrap().durable_batches.is_empty(),
            "Running B waits for the matching userMessage.clientId");
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"item/started","params":{"threadId":"thread-1","turnId":"turn-B",
                "item":{"id":"echo-1","type":"userMessage","clientId":"client-1","text":"Inspect only"}}
        })), 3).await.unwrap());
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"item/started","params":{"threadId":"thread-1","turnId":"turn-B",
                "item":{"id":"reply-1","type":"agentMessage","text":"Ready"}}
        })), 4).await.unwrap());
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-B","status":"completed"}}
        })), 5).await.unwrap());
        let states = shared.0.lock().unwrap().durable_batches.iter().flatten().filter_map(|event| {
            match event { DurableAdapterEvent::Turn { state, .. } => Some(*state), _ => None }
        }).collect::<Vec<_>>();
        assert_eq!(states, [TurnState::Running, TurnState::Completed]);
        assert!(shared.0.lock().unwrap().durable_batches.iter().flatten().any(|event| matches!(
            event,
            DurableAdapterEvent::Item { external_item_id, .. } if external_item_id.as_str() == "reply-1"
        )));
        assert_eq!(pump.intent.active_turn.external_turn_id.as_ref().unwrap().as_str(), "turn-B");
    });
}

fn goal_active_codex_pump() -> (AdapterPump<CodexRuntime, FakeJournal, Mapper>, Shared, u64) {
    let mut codex = CodexRuntime::fixture(Platform::WindowsX86_64);
    let init = codex.command(AdapterCommand::Initialize).unwrap();
    codex
        .resolve_egress(init.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = codex
        .ingest(
            &codex_frame(serde_json::json!({"id":1,"result":{
                "userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
                "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    codex
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let start = codex
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    codex
        .ingest(
            &codex_frame(serde_json::json!({
                "id": codex_request_id(&start),"result":{"thread":{"id":"thread-1"}}
            })),
            &Mapper,
        )
        .unwrap();
    codex.enable_observed_goal_for_test();
    let prompt = codex
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = codex_request_id(&prompt);
    let (pump, shared) = codex_pump(codex);
    (pump, shared, prompt_id)
}

struct CaptureTransport(Vec<u8>);

impl EgressTransport for CaptureTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0 = frame.payload().to_vec();
        FrameWriteOutcome::Complete
    }
}

#[test]
fn foreign_oversize_keeps_child_read_pending_until_actual_reply() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        pump.adapter.learn_child_for_test("child-1");
        let mut output = CaptureTransport(Vec::new());
        pump.write_child(&mut output, "child-1", "thread-1").unwrap();
        let request: serde_json::Value = serde_json::from_slice(&output.0).unwrap();
        let child_id = request["id"].as_u64().unwrap();
        assert_eq!(request["method"], "thread/read");
        assert_eq!(request["params"]["includeTurns"], true);
        let foreign = format!("{{\"id\":999,\"result\":\"{}\"}}\n", "x".repeat(1_048_576));
        for chunk in foreign.as_bytes().chunks(8 * 1024) {
            pump.push_stdout_chunk(chunk, 10).await.unwrap();
            while pump.has_pending_stdout_input() { pump.drain_stdout(10).await.unwrap(); }
        }
        assert!(pump.adapter.child_read_pending());
        let actual = codex_line(serde_json::json!({"id":child_id,"result":{"thread":{
            "id":"child-1","parentThreadId":"thread-1","turns":[]
        }}}));
        assert!(pump.push_stdout_chunk(&actual, 11).await.unwrap());
        assert!(matches!(pump.take_native_rpc_reply(), Some(crate::acp::goal::NativeRpcReply::ChildTranscript(snapshot))
            if snapshot.child_thread_id == "child-1" && snapshot.parent_thread_id == "thread-1"));
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
        })), 12).await.unwrap());
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
    });
}

#[test]
fn correlated_oversize_settles_too_large_and_tombstones_late_reply() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        pump.adapter.learn_child_for_test("child-1");
        let mut output = CaptureTransport(Vec::new());
        pump.write_child(&mut output, "child-1", "thread-1")
            .unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&output.0).unwrap()["id"]
            .as_u64()
            .unwrap();
        let oversize = format!("{{\"id\":{id},\"result\":\"{}\"}}\n", "x".repeat(1_048_576));
        for chunk in oversize.as_bytes().chunks(8 * 1024) {
            pump.push_stdout_chunk(chunk, 10).await.unwrap();
            while pump.has_pending_stdout_input() {
                pump.drain_stdout(10).await.unwrap();
            }
        }
        assert!(!pump.adapter.child_read_pending());
        assert_eq!(
            pump.take_native_rpc_reply(),
            Some(crate::acp::goal::NativeRpcReply::TooLarge)
        );
        assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
            "id":id,"result":{"thread":{"id":"child-1","parentThreadId":"thread-1","turns":[]}}
        })), 11).await.unwrap());
        assert!(pump.take_native_rpc_reply().is_none());
        assert!(
            pump.push_stdout_chunk(
                &codex_line(serde_json::json!({
                    "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
                })),
                12
            )
            .await
            .unwrap()
        );
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
    });
}

#[test]
fn goal_active_mismatch_rejects_unverified_first_item_without_durable_identity() {
    runtime().block_on(async {
        for item in [
            serde_json::json!({"id":"echo-1","type":"userMessage","clientId":"other-client"}),
            serde_json::json!({"id":"echo-1","type":"userMessage"}),
            serde_json::json!({"id":"item-1","type":"reasoning","summary":["unverified"]}),
        ] {
            let (mut pump, shared, prompt_id) = goal_active_codex_pump();
            pump.push_stdout_chunk(&codex_line(serde_json::json!({
                "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
            })), 1).await.unwrap();
            pump.push_stdout_chunk(&codex_line(serde_json::json!({
                "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-B"}}
            })), 2).await.unwrap();
            assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
                "method":"item/started","params":{"threadId":"thread-1","turnId":"turn-B","item":item}
            })), 3).await.is_err());
            assert!(pump.teardown_required());
            assert!(shared.0.lock().unwrap().durable_batches.is_empty());
        }
    });
}

#[test]
fn goal_active_mismatch_rejects_a_second_started_turn_before_verification() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
            })),
            1,
        )
        .await
        .unwrap();
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-B"}}
            })),
            2,
        )
        .await
        .unwrap();
        assert!(matches!(
            pump.push_stdout_chunk(
                &codex_line(serde_json::json!({
                    "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-C"}}
                })),
                3
            )
            .await,
            Err(AdapterPumpError::Adapter(AdapterRuntimeError::Operation))
        ));
        assert!(pump.teardown_required());
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
    });
}

#[test]
fn goal_active_provisional_turn_expires_without_another_wire_frame() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
            })),
            1,
        )
        .await
        .unwrap();
        assert_eq!(pump.next_deferred_flush_deadline_millis(), Some(10_001));
        assert!(!pump.flush_deferred_if_needed(10_000).await.unwrap());
        assert!(matches!(
            pump.flush_deferred_if_needed(10_001).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
    });
}

#[test]
fn goal_active_provisional_turn_refuses_steer_until_client_verification() {
    runtime().block_on(async {
        let (mut pump, shared, prompt_id) = goal_active_codex_pump();
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "id":prompt_id,"result":{"turn":{"id":"turn-A"}}
            })),
            1,
        )
        .await
        .unwrap();
        assert!(matches!(
            pump.queue_steer(
                Prompt::new("prompt", "Change direction").unwrap(),
                RequiredText::new("client", "steer-1").unwrap(),
            ),
            Err(AdapterPumpError::Intent)
        ));
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-B"}}
            })),
            2,
        )
        .await
        .unwrap();
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "method":"item/started","params":{"threadId":"thread-1","turnId":"turn-B",
                    "item":{"id":"echo-1","type":"userMessage","clientId":"client-1"}}
            })),
            3,
        )
        .await
        .unwrap();
        assert!(
            pump.queue_steer(
                Prompt::new("prompt", "Change direction").unwrap(),
                RequiredText::new("client", "steer-2").unwrap(),
            )
            .is_ok()
        );
    });
}

fn codex_line(value: serde_json::Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn codex_frame(value: serde_json::Value) -> Frame {
    let mut decoder = IncrementalFrameDecoder::new();
    decoder.push_chunk(&codex_line(value)).unwrap().unwrap()
}

fn codex_request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<serde_json::Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn codex_pump(codex: CodexRuntime) -> (AdapterPump<CodexRuntime, FakeJournal, Mapper>, Shared) {
    let shared = Shared(Arc::new(Mutex::new(FakeState::default())));
    let publisher: Arc<dyn ProjectionPublisher> = Arc::new(FakePublisher {
        shared: shared.clone(),
        outcomes: Mutex::new(VecDeque::new()),
    });
    let mut scope = session_intent();
    scope.binding_digest = [9; 32];
    scope.session_id = SageAgentSessionId::parse("session-codex-runtime").unwrap();
    scope.external_session_id = ExternalId::new("session", "thread-1").unwrap();
    let pump = AdapterPump::new(
        codex,
        FakeJournal {
            shared: shared.clone(),
            outcomes: VecDeque::new(),
            allow_rebind: true,
            next_projection: 0,
            turn_id: turn("one"),
            turn_state: TurnState::Sending,
            turn_revision: 1,
        },
        Mapper,
        publisher,
        scope.bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: None,
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
        }),
    )
    .unwrap();
    (pump, shared)
}

#[derive(Default)]
struct FakeState {
    batches: VecDeque<AdapterBatch>,
    commands: Vec<&'static str>,
    resolutions: Vec<EgressResolution>,
    durable_batches: Vec<Vec<DurableAdapterEvent>>,
    request_states: Vec<ServerRequestState>,
    resolved_request_ids: Vec<String>,
    permission_failure: Option<(ServerRequestState, PumpJournalError)>,
    log: Vec<&'static str>,
    ingest_error: Option<AdapterRuntimeError>,
    refusal_detail: Option<RefusalDetail>,
}

#[derive(Clone)]
struct Shared(Arc<Mutex<FakeState>>);

struct FakeAdapter {
    shared: Shared,
    next_request: u64,
}

impl HarnessAdapter for FakeAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn refusal_detail(&self) -> Option<RefusalDetail> {
        self.shared.0.lock().unwrap().refusal_detail.clone()
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let AdapterCommand::ResolvePermission {
            request_id: resolved_request_id,
            decision,
        } = command
        {
            self.shared
                .0
                .lock()
                .unwrap()
                .resolved_request_ids
                .push(resolved_request_id.as_str().to_owned());
            let request_id = ExternalId::new("request", "external-request")
                .map_err(|_| AdapterRuntimeError::Frame)?;
            let token = match decision {
                crate::acp::runtime::permissions::PermissionDecision::Ask => {
                    EgressToken::protocol_notification()
                }
                crate::acp::runtime::permissions::PermissionDecision::AllowOnce
                | crate::acp::runtime::permissions::PermissionDecision::AllowAlways => {
                    EgressToken::permission_allow(request_id)
                }
                crate::acp::runtime::permissions::PermissionDecision::Deny => {
                    EgressToken::mandatory_deny(request_id, false)
                }
            };
            self.shared.0.lock().unwrap().commands.push("permission");
            return Ok(AdapterEgress::new(
                OutboundFrame::new(br#"{\"permission\":true}"#.to_vec())
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                token,
            ));
        }
        let cancel = matches!(command, AdapterCommand::Cancel);
        let name = if cancel { "cancel" } else { "command" };
        self.shared.0.lock().unwrap().commands.push(name);
        self.next_request += 1;
        let token = if cancel {
            EgressToken::cancel_request(self.next_request)
        } else {
            EgressToken::request(self.next_request)
        };
        Ok(AdapterEgress::new(
            OutboundFrame::new(format!(r#"{{"id":{}}}"#, self.next_request).into_bytes()).unwrap(),
            token,
        ))
    }

    fn cancel_permission(
        &mut self,
        request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.shared.0.lock().unwrap().log.push("prepareCancel");
        let external = ExternalId::new("permissionRequest", request_id.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        let frame = OutboundFrame::new(
            format!(
                r#"{{"jsonrpc":"2.0","id":"{}","result":{{"outcome":{{"outcome":"cancelled"}}}}}}"#,
                request_id.as_str()
            )
            .into_bytes(),
        )
        .map_err(|_| AdapterRuntimeError::Frame)?;
        Ok(AdapterEgress::new(
            frame,
            EgressToken::permission_cancel(external),
        ))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut state = self.shared.0.lock().unwrap();
        if let Some(error) = state.ingest_error.take() {
            return Err(error);
        }
        state
            .batches
            .pop_front()
            .ok_or(AdapterRuntimeError::Operation)
    }

    fn commit_mandatory_egress(&mut self, _: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.shared.0.lock().unwrap().log.push("mandatory");
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        _: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.shared.0.lock().unwrap().resolutions.push(resolution);
        Ok(())
    }
}

struct FakeJournal {
    shared: Shared,
    outcomes: VecDeque<Result<ProjectionRevision, PumpJournalError>>,
    allow_rebind: bool,
    next_projection: u64,
    turn_id: SageTurnId,
    turn_state: TurnState,
    turn_revision: u64,
}

impl AdapterBatchJournal for FakeJournal {
    fn commit<'a>(
        &'a mut self,
        events: Vec<DurableAdapterEvent>,
        _: usize,
    ) -> Pin<Box<dyn Future<Output = Result<JournalReceipt, PumpJournalError>> + Send + 'a>> {
        self.next_projection += 1;
        let outcome = self
            .outcomes
            .pop_front()
            .unwrap_or_else(|| Ok(revision(self.next_projection)));
        if let Ok(projection) = outcome {
            self.next_projection = projection.get();
        }
        for event in &events {
            if let DurableAdapterEvent::Turn { state, .. } = event
                && *state != self.turn_state
            {
                self.turn_state = *state;
                self.turn_revision += 1;
            }
        }
        let turn_id = self.turn_id.clone();
        let turn_state = self.turn_state;
        let turn_revision = self.turn_revision;
        let mut state = self.shared.0.lock().unwrap();
        state.log.push("journal");
        state.durable_batches.push(events);
        drop(state);
        Box::pin(async move {
            outcome.map(|projection_revision| {
                JournalReceipt::committed(
                    projection_revision,
                    AdapterTurnCursor::test_only(
                        turn_id,
                        turn_state,
                        DeliveryState::Correlated,
                        EntityRevision::new(turn_revision).unwrap(),
                        projection_revision,
                    ),
                )
            })
        })
    }

    fn transition_permission<'a>(
        &'a mut self,
        _: &'a PermissionIntent,
        request_state: ServerRequestState,
        _: i64,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectionRevision, PumpJournalError>> + Send + 'a>>
    {
        self.next_projection += 1;
        let revision = revision(self.next_projection);
        let label = match request_state {
            ServerRequestState::ResponseQueued => "responseQueued",
            ServerRequestState::ResolvedDeclined => "resolvedDeclined",
            ServerRequestState::Cancelled => "cancelled",
            _ => "unexpectedRequestState",
        };
        let mut state = self.shared.0.lock().unwrap();
        state.log.push(label);
        if state
            .permission_failure
            .is_some_and(|(failed, _)| failed == request_state)
        {
            let error = state.permission_failure.take().unwrap().1;
            return Box::pin(async move { Err(error) });
        }
        state.request_states.push(request_state);
        Box::pin(async move { Ok(revision) })
    }

    fn try_rebind(mut self, intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)> {
        if self.allow_rebind {
            self.turn_id = intent.active_turn.turn_id;
            self.turn_state = TurnState::Sending;
            self.turn_revision = 1;
            Ok(self)
        } else {
            Err((self, PumpJournalError::Rejected))
        }
    }
}

struct FakePublisher {
    shared: Shared,
    outcomes: Mutex<VecDeque<Result<ProjectionPublishStatus, ProjectionPublishError>>>,
}

impl ProjectionPublisher for FakePublisher {
    fn publish_committed(
        &self,
        _: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        self.shared.0.lock().unwrap().log.push("publish");
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(ProjectionPublishStatus::Published))
    }
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

struct ScriptTransport(VecDeque<FrameWriteOutcome>);

impl EgressTransport for ScriptTransport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.0.pop_front().unwrap()
    }
}

#[derive(Default)]
struct CountingTransport(usize);

impl EgressTransport for CountingTransport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.0 += 1;
        FrameWriteOutcome::Complete
    }
}

#[test]
fn normalized_events_commit_once_before_one_projection_publish() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![owned(
            vec![item("item-one"), item("item-two")],
            Vec::new(),
        )]);
        assert!(pump.push_stdout_chunk(b"{}\n", 11).await.unwrap());
        let state = shared.0.lock().unwrap();
        assert_eq!(state.log, ["journal", "publish"]);
        assert_eq!(state.durable_batches.len(), 1);
        assert_eq!(state.durable_batches[0].len(), 2);
        assert!(state.durable_batches[0].iter().all(|event| {
            matches!(event, DurableAdapterEvent::Item { scope, now_millis: 11, .. }
                if scope.binding_digest == "07".repeat(32))
        }));
    });
}

#[test]
fn activity_child_normalizes_atomically_and_foreign_parent_identity_is_ignored() {
    runtime().block_on(async {
        let mut foreign_parent = identity("foreign-parent");
        foreign_parent.binding_digest = [8; 32];
        let (mut pump, shared) = build_pump(vec![owned(
            vec![
                activity_item(identity("parent")),
                activity_item(foreign_parent),
            ],
            Vec::new(),
        )]);

        assert!(pump.push_stdout_chunk(b"{}\n", 11).await.unwrap());
        let state = shared.0.lock().unwrap();
        assert!(matches!(
            state.durable_batches.as_slice(),
            [batch] if matches!(
                batch.as_slice(),
                [DurableAdapterEvent::ActivityItem(activity)] if matches!(
                    activity.as_ref(),
                    DurableActivityItem {
                        external_activity_id,
                        external_item_id,
                        parent_external_item_id,
                        role: ActivityRole::Child,
                        depth: 1,
                        observed_sequence: 1,
                        payload: NormalizedPayload::Text { text },
                        ..
                    } if external_activity_id.as_str() == "child-activity"
                    && external_item_id.as_str() == "child-item"
                    && parent_external_item_id.as_str() == "parent"
                    && text.as_str() == "child text"
                )
            )
        ));
    });
}

#[test]
fn bounded_reader_ignores_foreign_late_and_duplicate_terminal_events() {
    runtime().block_on(async {
        let mut foreign = identity("foreign-item");
        foreign.binding_digest = [8; 32];
        let (mut pump, shared) = build_pump(vec![
            owned(
                vec![AdapterEvent::Item {
                    id: foreign,
                    lifecycle: ItemLifecycle::Streaming,
                    tool_state: None,
                    payload: text("foreign"),
                }],
                Vec::new(),
            ),
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(
                vec![terminal(TurnState::Completed), item("late-item")],
                Vec::new(),
            ),
        ]);
        assert!(pump.push_stdout_chunk(b"{}\r\n{}\n{}\n", 12).await.unwrap());
        assert!(pump.drain_stdout(13).await.unwrap());
        assert!(pump.drain_stdout(14).await.unwrap());
        assert!(!pump.drain_stdout(15).await.unwrap());
        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), 1);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }]
        ));
    });
}

#[test]
fn first_correlated_adapter_event_binds_external_turn_identity_once() {
    runtime().block_on(async {
        let first = identity("first-item");
        let mut alias = identity("alias-item");
        alias.turn_id = Some(ExternalId::new("turn", "foreign-turn").unwrap());
        let (mut pump, shared) = build_pump(vec![
            owned(
                vec![AdapterEvent::Item {
                    id: first,
                    lifecycle: ItemLifecycle::Streaming,
                    tool_state: None,
                    payload: text("first"),
                }],
                Vec::new(),
            ),
            owned(
                vec![AdapterEvent::Item {
                    id: alias,
                    lifecycle: ItemLifecycle::Streaming,
                    tool_state: None,
                    payload: text("foreign"),
                }],
                Vec::new(),
            ),
        ]);
        pump.intent.active_turn.external_turn_id = None;

        assert!(pump.push_stdout_chunk(b"{}\n{}\n", 12).await.unwrap());
        assert!(pump.drain_stdout(13).await.unwrap());
        assert_eq!(
            pump.intent
                .active_turn
                .external_turn_id
                .as_ref()
                .map(ExternalId::as_str),
            Some("external-turn")
        );
        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), 1);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::Item {
                external_item_id,
                ..
            }] if external_item_id.as_str() == "first-item"
        ));
    });
}

#[test]
fn store_minted_turn_identity_starts_without_an_external_alias() {
    let intent = ActiveTurnIntent::store_minted(turn("native-authority")).unwrap();
    assert_eq!(intent.turn_id, turn("native-authority"));
    assert_eq!(intent.client_message_id.as_str(), "turn-native-authority");
    assert_eq!(intent.external_turn_id, None);
}

#[test]
fn steer_acknowledgment_waits_for_the_exact_response_and_rejects_late_ids() {
    runtime().block_on(async {
        let (mut pump, _) = build_pump(vec![
            owned(
                vec![AdapterEvent::SteerResponse {
                    request_id: 99,
                    accepted: true,
                }],
                Vec::new(),
            ),
            owned(
                vec![AdapterEvent::SteerResponse {
                    request_id: 1,
                    accepted: false,
                }],
                Vec::new(),
            ),
        ]);
        let (_, request_id) = pump
            .queue_steer(
                Prompt::new("prompt", "Change direction").unwrap(),
                RequiredText::new("client", "steer-one").unwrap(),
            )
            .unwrap();
        assert_eq!(request_id, 1);
        let mut transport = CountingTransport::default();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        assert_eq!(pump.take_steer_response(request_id), None);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        assert_eq!(pump.take_steer_response(request_id), None);
        pump.push_stdout_chunk(b"{}\n", 2).await.unwrap();
        assert_eq!(pump.take_steer_response(request_id), Some(false));
        assert_eq!(pump.take_steer_response(request_id), None);
    });
}

#[test]
fn queue_backpressure_and_write_evidence_are_distinct() {
    let (mut pump, shared) = pump_with_queue(Vec::new(), queue_with(1, 64, 0, 0));
    assert_eq!(
        pump.queue_command(AdapterCommand::Initialize).unwrap(),
        QueueDisposition::Queued
    );
    assert!(matches!(
        pump.queue_command(AdapterCommand::Initialize),
        Err(AdapterPumpError::Backpressure)
    ));
    let mut transport = ScriptTransport(VecDeque::from([FrameWriteOutcome::ZeroByteFailure]));
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        FlushDisposition::ZeroBytes
    );
    pump.queue_command(AdapterCommand::Initialize).unwrap();
    let mut complete = ScriptTransport(VecDeque::from([FrameWriteOutcome::Complete]));
    assert_eq!(
        pump.flush_one(&mut complete).unwrap(),
        FlushDisposition::Written
    );
    assert_eq!(
        shared.0.lock().unwrap().resolutions,
        [
            EgressResolution::Queued,
            EgressResolution::QueueRejected,
            EgressResolution::WriteZeroBytes,
            EgressResolution::Queued,
            EgressResolution::Written
        ]
    );

    let (mut pump, shared) = build_pump(Vec::new());
    pump.queue_command(AdapterCommand::Initialize).unwrap();
    let mut transport = ScriptTransport(VecDeque::from([FrameWriteOutcome::DeliveryUnknown {
        bytes_written: 2,
    }]));
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        FlushDisposition::DeliveryUnknown
    );
    assert_eq!(
        shared.0.lock().unwrap().resolutions.last(),
        Some(&EgressResolution::DeliveryUnknown)
    );
    assert!(pump.teardown_required());
}

#[test]
fn delivery_unknown_reports_once_then_rejects_every_later_operation() {
    runtime().block_on(async {
        let (mut pump, _) = build_pump(Vec::new());
        pump.queue_command(AdapterCommand::Initialize).unwrap();
        let mut unknown = ScriptTransport(VecDeque::from([FrameWriteOutcome::DeliveryUnknown {
            bytes_written: 1,
        }]));
        assert_eq!(
            pump.flush_one(&mut unknown).unwrap(),
            FlushDisposition::DeliveryUnknown
        );
        assert!(pump.teardown_required());
        assert!(matches!(
            pump.queue_command(AdapterCommand::Initialize),
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.flush_one(&mut unknown),
            Err(AdapterPumpError::TeardownRequired)
        ));
    });
}

#[test]
fn permission_ingress_journals_a_request_and_waits_for_a_later_decision() {
    runtime().block_on(async {
        let event = AdapterEvent::PermissionRequest {
            turn_id: identity("turn-envelope"),
            item_id: identity("permission-item"),
            external_request_id: ExternalId::new("permissionRequest", "request-external").unwrap(),
            payload: permission(),
            request_payload_digest: [1; 32],
        };
        let (mut pump, shared) = build_pump(vec![owned(vec![event], Vec::new())]);
        pump.push_stdout_chunk(b"{}\n", 20).await.unwrap();
        let state = shared.0.lock().unwrap();
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::PermissionRequest { intent, now_millis: 20 }]
                if intent.binding_digest == "07".repeat(32)
                    && intent.session_id == session("one")
                    && intent.turn_id == turn("one")
                    && intent.runtime_epoch == RuntimeEpoch::new(1).unwrap()
                    && intent.request_id.as_str().starts_with("permission:")
                    && intent.request_id.as_str() != "request-external"
                    && intent.external_request_id.as_str() == "request-external"
        ));
        assert!(state.request_states.is_empty());
        assert!(state.commands.is_empty());
        drop(state);
        let mut transport = CountingTransport::default();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Idle
        );
        assert_eq!(transport.0, 0);
    });
}

fn permission_id_for_binding_session(
    binding_digest: [u8; 32],
    session_name: &str,
    external_request: &str,
) -> (SageServerRequestId, ExternalId) {
    let mut item_id = identity("permission-item");
    item_id.binding_digest = binding_digest;
    item_id.session_id = session(session_name);
    let mut turn_id = identity("turn-envelope");
    turn_id.binding_digest = binding_digest;
    turn_id.session_id = session(session_name);
    let event = AdapterEvent::PermissionRequest {
        turn_id,
        item_id,
        external_request_id: ExternalId::new("permissionRequest", external_request).unwrap(),
        payload: permission(),
        request_payload_digest: [1; 32],
    };
    let mut intent = session_intent();
    intent.binding_digest = binding_digest;
    intent.session_id = session(session_name);
    let (mut pump, shared) = pump_configured_for_scope(
        vec![owned(vec![event], Vec::new())],
        VecDeque::new(),
        VecDeque::new(),
        queue_with(
            LIMITS.queues.egress_frames as usize,
            LIMITS.queues.egress_bytes as usize,
            LIMITS.queues.egress_control_frames_inside_total as usize,
            LIMITS.queues.egress_control_bytes_inside_total as usize,
        ),
        intent,
    );
    runtime()
        .block_on(pump.push_stdout_chunk(b"{}\n", 20))
        .unwrap();
    let state = shared.0.lock().unwrap();
    match &state.durable_batches[0][0] {
        DurableAdapterEvent::PermissionRequest { intent, .. } => (
            intent.request_id.clone(),
            intent.external_request_id.clone(),
        ),
        _ => panic!("permission request must be durable"),
    }
}

#[test]
fn claude_reused_wire_permission_id_is_distinct_across_sessions() {
    let first = permission_id_for_binding_session([0x11; 32], "claude-one", "string:1");
    let second = permission_id_for_binding_session([0x11; 32], "claude-two", "string:1");
    assert_ne!(first.0, second.0);
    assert_eq!(first.1.as_str(), "string:1");
    assert_eq!(second.1.as_str(), "string:1");
}

#[test]
fn codex_reused_wire_permission_id_is_distinct_across_sessions() {
    let first = permission_id_for_binding_session([0x22; 32], "codex-one", "number:0");
    let second = permission_id_for_binding_session([0x22; 32], "codex-two", "number:0");
    assert_ne!(first.0, second.0);
    assert_eq!(first.1.as_str(), "number:0");
    assert_eq!(second.1.as_str(), "number:0");
}

#[test]
fn permission_request_ignores_an_unpaired_mandatory_deny_egress() {
    runtime().block_on(async {
        let event = AdapterEvent::PermissionRequest {
            turn_id: identity("turn-envelope"),
            item_id: identity("permission-item"),
            external_request_id: ExternalId::new("permissionRequest", "request-external").unwrap(),
            payload: permission(),
            request_payload_digest: [1; 32],
        };
        let stale_deny = AdapterEgress::new(
            OutboundFrame::new(br#"{\"result\":{\"decision\":\"decline\"}}"#.to_vec()).unwrap(),
            EgressToken::mandatory_deny(
                ExternalId::new("permissionRequest", "leftover-deny").unwrap(),
                false,
            ),
        );
        let (mut pump, shared) = build_pump(vec![owned(vec![event], vec![stale_deny])]);

        pump.push_stdout_chunk(b"{}\n", 20).await.unwrap();

        let (persisted_request, journaled, no_resolutions) = {
            let state = shared.0.lock().unwrap();
            (
                matches!(
                    state.durable_batches[0].as_slice(),
                    [DurableAdapterEvent::PermissionRequest { .. }]
                ),
                state.log == ["journal", "publish"],
                state.resolutions.is_empty(),
            )
        };
        assert!(persisted_request);
        assert!(journaled);
        assert!(no_resolutions);
        pump.enqueue(AdapterEgress::new(
            OutboundFrame::new(br#"{\"result\":{\"decision\":\"decline\"}}"#.to_vec()).unwrap(),
            EgressToken::mandatory_deny(
                ExternalId::new("permissionRequest", "leftover-deny").unwrap(),
                false,
            ),
        ))
        .unwrap();
        let mut transport = CountingTransport::default();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        assert_eq!(transport.0, 1);
        assert!(!pump.teardown_required());
    });
}

#[test]
fn ask_does_not_invoke_the_adapter_or_write_a_harness_byte() {
    runtime().block_on(async {
        let (mut pump, shared) = permission_pump();
        pump.push_stdout_chunk(b"{}\n", 20).await.unwrap();
        let request_id = permission_request_id(&shared);

        pump.resolve_permission(
            &request_id,
            crate::acp::runtime::permissions::PermissionDecision::Ask,
            21,
        )
        .await
        .unwrap();

        let mut transport = CountingTransport::default();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Idle
        );
        assert_eq!(transport.0, 0);
        assert!(shared.0.lock().unwrap().commands.is_empty());
    });
}

#[test]
fn later_deny_resolves_only_after_complete_write() {
    runtime().block_on(async {
        let (mut pump, shared) = permission_pump();
        pump.push_stdout_chunk(b"{}\n", 20).await.unwrap();
        let request_id = match &shared.0.lock().unwrap().durable_batches[0][0] {
            DurableAdapterEvent::PermissionRequest { intent, .. } => intent.request_id.clone(),
            _ => panic!("permission request must be durable before resolution"),
        };
        pump.resolve_permission(
            &request_id,
            crate::acp::runtime::permissions::PermissionDecision::Deny,
            21,
        )
        .await
        .unwrap();
        let mut transport = ScriptTransport(VecDeque::from([FrameWriteOutcome::Complete]));
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        assert!(pump.settle_written_permission(22).await.unwrap());
        let state = shared.0.lock().unwrap();
        assert_eq!(
            state.request_states,
            [
                ServerRequestState::DeclineCommitted,
                ServerRequestState::ResponseQueued,
                ServerRequestState::ResolvedDeclined
            ]
        );
        assert_eq!(state.commands, ["permission"]);
        assert_eq!(state.resolved_request_ids, ["external-request"]);
    });
}

#[test]
fn mandatory_deny_state_failures_block_or_tear_down_at_the_exact_boundary() {
    runtime().block_on(async {
        let (mut queue_failure, shared) = permission_pump();
        queue_failure.push_stdout_chunk(b"{}\n", 20).await.unwrap();
        let request_id = permission_request_id(&shared);
        shared.0.lock().unwrap().permission_failure = Some((
            ServerRequestState::ResponseQueued,
            PumpJournalError::Rejected,
        ));
        assert!(matches!(
            queue_failure
                .resolve_permission(
                    &request_id,
                    crate::acp::runtime::permissions::PermissionDecision::Deny,
                    21,
                )
                .await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        let mut queued = CountingTransport::default();
        assert!(matches!(
            queue_failure.flush_one(&mut queued),
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert_eq!(queued.0, 0);
        assert_eq!(
            shared.0.lock().unwrap().request_states,
            [ServerRequestState::DeclineCommitted]
        );

        let (mut resolution_failure, shared) = permission_pump();
        resolution_failure
            .push_stdout_chunk(b"{}\n", 20)
            .await
            .unwrap();
        let request_id = permission_request_id(&shared);
        resolution_failure
            .resolve_permission(
                &request_id,
                crate::acp::runtime::permissions::PermissionDecision::Deny,
                21,
            )
            .await
            .unwrap();
        let mut written = CountingTransport::default();
        assert_eq!(
            resolution_failure.flush_one(&mut written).unwrap(),
            FlushDisposition::Written
        );
        shared.0.lock().unwrap().permission_failure = Some((
            ServerRequestState::ResolvedDeclined,
            PumpJournalError::Rejected,
        ));
        assert!(matches!(
            resolution_failure.settle_written_permission(21).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert_eq!(written.0, 1);
        assert_eq!(
            shared.0.lock().unwrap().request_states,
            [
                ServerRequestState::DeclineCommitted,
                ServerRequestState::ResponseQueued
            ]
        );
    });
}

#[test]
fn paired_deny_ingress_does_not_tear_down_the_common_pump() {
    runtime().block_on(async {
        let permission_event = AdapterEvent::PermissionRequest {
            turn_id: identity("turn-envelope"),
            item_id: identity("external-request"),
            external_request_id: ExternalId::new("permissionRequest", "external-request").unwrap(),
            payload: permission(),
            request_payload_digest: [1; 32],
        };
        let paired_deny = AdapterEgress::new(
            OutboundFrame::new(br#"{\"result\":{\"decision\":\"decline\"}}"#.to_vec()).unwrap(),
            EgressToken::mandatory_deny(
                ExternalId::new("permissionRequest", "external-request").unwrap(),
                false,
            ),
        );
        let (mut pump, shared) = build_pump(vec![owned(vec![permission_event], vec![paired_deny])]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let (persisted_request, journaled, no_resolutions) = {
            let state = shared.0.lock().unwrap();
            (
                matches!(
                    state.durable_batches[0].as_slice(),
                    [DurableAdapterEvent::PermissionRequest { .. }]
                ),
                state.log == ["journal", "publish"],
                state.resolutions.is_empty(),
            )
        };
        assert!(persisted_request);
        assert!(journaled);
        assert!(no_resolutions);
        pump.enqueue(AdapterEgress::new(
            OutboundFrame::new(br#"{\"result\":{\"decision\":\"decline\"}}"#.to_vec()).unwrap(),
            EgressToken::mandatory_deny(
                ExternalId::new("permissionRequest", "external-request").unwrap(),
                false,
            ),
        ))
        .unwrap();
        let mut transport = CountingTransport::default();
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        assert_eq!(transport.0, 1);
        assert!(!pump.teardown_required());
        assert!(!pump.settle_written_permission(2).await.unwrap());
    });
}

#[test]
fn cancellation_requires_the_exact_live_turn_tuple() {
    let (mut pump, shared) = build_pump(Vec::new());
    assert!(matches!(
        pump.queue_cancel_notification(
            &session("foreign"),
            RuntimeEpoch::new(1).unwrap(),
            &turn("one"),
            &message()
        ),
        Err(AdapterPumpError::NoActiveTurn)
    ));
    assert!(shared.0.lock().unwrap().commands.is_empty());
    assert_eq!(
        pump.queue_cancel_notification(
            &session("one"),
            RuntimeEpoch::new(1).unwrap(),
            &turn("one"),
            &message()
        )
        .unwrap(),
        QueueDisposition::Queued
    );
    assert_eq!(shared.0.lock().unwrap().commands, ["cancel"]);
}

#[test]
fn data_saturation_reserves_the_control_lane_for_cancel() {
    let (mut pump, shared) = pump_with_queue(Vec::new(), queue_with(2, 128, 1, 64));
    pump.queue_command(AdapterCommand::Initialize).unwrap();
    assert!(matches!(
        pump.queue_command(AdapterCommand::Initialize),
        Err(AdapterPumpError::Backpressure)
    ));
    assert_eq!(
        pump.queue_cancel_notification(
            &session("one"),
            RuntimeEpoch::new(1).unwrap(),
            &turn("one"),
            &message()
        )
        .unwrap(),
        QueueDisposition::Queued
    );
    assert_eq!(
        shared.0.lock().unwrap().commands,
        ["command", "command", "cancel"]
    );
}

#[test]
fn cancellation_is_rejected_after_the_durable_terminal() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![owned(
            vec![terminal(TurnState::Completed)],
            Vec::new(),
        )]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        assert!(matches!(
            pump.queue_cancel_notification(
                &session("one"),
                RuntimeEpoch::new(1).unwrap(),
                &turn("one"),
                &message()
            ),
            Err(AdapterPumpError::NoActiveTurn)
        ));
        assert!(shared.0.lock().unwrap().commands.is_empty());
    });
}

#[test]
fn unknown_commit_and_projection_recovery_are_typed_fail_closed_boundaries() {
    runtime().block_on(async {
        let (mut pump, shared) = pump_configured(
            vec![owned(vec![item("one")], Vec::new())],
            VecDeque::from([Err(PumpJournalError::CommitUnknown)]),
            VecDeque::new(),
            queue_with(8, 1024, 2, 256),
        );
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::Journal(PumpJournalError::CommitUnknown))
        ));
        assert!(pump.teardown_required());
        assert_eq!(shared.0.lock().unwrap().log, ["journal"]);

        let (mut pump, shared) = pump_configured(
            vec![owned(vec![item("one")], Vec::new())],
            VecDeque::new(),
            VecDeque::from([Ok(ProjectionPublishStatus::RecoveryRequired)]),
            queue_with(8, 1024, 2, 256),
        );
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::RecoveryRequired)
        ));
        assert_eq!(shared.0.lock().unwrap().log, ["journal", "publish"]);

        let (mut pump, _) = pump_configured(
            vec![owned(vec![item("one")], Vec::new())],
            VecDeque::new(),
            VecDeque::from([Ok(ProjectionPublishStatus::BootstrapRequired)]),
            queue_with(8, 1024, 2, 256),
        );
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::BootstrapRequired)
        ));

        let (mut pump, _) = pump_configured(
            vec![owned(vec![terminal(TurnState::Completed)], Vec::new())],
            VecDeque::new(),
            VecDeque::from([Ok(ProjectionPublishStatus::RecoveryRequired)]),
            queue_with(8, 1024, 2, 256),
        );
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::RecoveryRequired)
        ));
        assert!(!pump.completed_turn_ready());
        let durable = pump
            .latest_durable_turn_cursor()
            .expect("terminal commit proof survives publication failure");
        assert_eq!(durable.turn_id(), &turn("one"));
        assert_eq!(durable.state(), TurnState::Completed);
    });
}

#[test]
fn rejected_config_mutation_is_nonfatal_and_pump_remains_usable() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![owned(vec![item("after-rejection")], Vec::new())]);
        shared.0.lock().unwrap().ingest_error = Some(AdapterRuntimeError::ConfigMutationRejected);

        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::Refused(NonfatalRefusal::ConfigMutation))
        ));
        assert!(!pump.teardown_required());

        assert!(pump.push_stdout_chunk(b"{}\n", 2).await.unwrap());
        assert!(!pump.teardown_required());
        assert!(matches!(
            shared.0.lock().unwrap().durable_batches.as_slice(),
            [batch] if matches!(batch.as_slice(), [DurableAdapterEvent::Item { .. }])
        ));
    });
}

#[test]
fn idle_config_rejection_drains_following_frame_and_stays_aligned() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(vec![AdapterEvent::IgnoredLate], Vec::new()),
            owned(vec![AdapterEvent::IgnoredLate], Vec::new()),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let mut parts = match pump.try_into_completed_parts() {
            Ok(parts) => parts,
            Err(_) => panic!("completed parts"),
        };
        shared.0.lock().unwrap().ingest_error = Some(AdapterRuntimeError::ConfigMutationRejected);

        assert!(matches!(
            parts
                .accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut CaptureTransport(Vec::new()))
                .await,
            Err(AdapterPumpError::Refused(NonfatalRefusal::ConfigMutation))
        ));
        assert!(parts.idle_frame_aligned());

        assert!(
            parts
                .accept_idle_stdout_chunk(b"{}\n", 3, &mut CaptureTransport(Vec::new()))
                .await
                .is_ok()
        );
        assert!(parts.idle_frame_aligned());
    });
}

#[test]
fn consumed_failures_and_protocol_violations_latch_teardown() {
    runtime().block_on(async {
        let (mut pump, _) = pump_configured(
            vec![owned(vec![item("one")], Vec::new())],
            VecDeque::from([Err(PumpJournalError::Rejected)]),
            VecDeque::new(),
            queue_with(8, 1024, 2, 256),
        );
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.queue_command(AdapterCommand::Initialize),
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));

        let (mut pump, _) = build_pump(Vec::new());
        assert!(matches!(
            pump.push_stdout_chunk(b"{]\n", 1).await,
            Err(AdapterPumpError::Frame)
        ));
        assert!(matches!(
            pump.queue_command(AdapterCommand::Initialize),
            Err(AdapterPumpError::TeardownRequired)
        ));

        let (mut pump, _) = build_pump(Vec::new());
        assert!(matches!(
            pump.push_stdout_chunk(b"{\0}\n", 1).await,
            Err(AdapterPumpError::Frame)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));

        let (mut pump, shared) = build_pump(Vec::new());
        shared.0.lock().unwrap().ingest_error = Some(AdapterRuntimeError::Frame);
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 1).await,
            Err(AdapterPumpError::Frame)
        ));
        assert!(matches!(
            pump.queue_command(AdapterCommand::Initialize),
            Err(AdapterPumpError::TeardownRequired)
        ));
    });
}

#[test]
fn readers_preserve_exact_teardown_error_after_latch() {
    runtime().block_on(async {
        let (mut pump, _) = build_pump(Vec::new());
        assert!(matches!(
            pump.push_stdout_chunk(b"{]\n", 1).await,
            Err(AdapterPumpError::Frame)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.drain_stdout(2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.finish_stdout(2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.push_stdout_chunk_deferred(b"{}\n", 2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.drain_stdout_deferred(2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(matches!(
            pump.finish_stdout_deferred(2).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
    });
}

#[test]
fn adapter_contract_rejects_count_overflow_before_the_pump() {
    let events = (0..=LIMITS.queues.journal_facts_per_transaction)
        .map(|_| AdapterEvent::Acknowledged)
        .collect();
    assert!(matches!(
        AdapterBatch::new(events, Vec::new()),
        Err(AdapterRuntimeError::BatchCount)
    ));
}

#[test]
fn deferred_snapshots_coalesce_by_identity_and_publish_once() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![item_with_payload("one", text("one-a"))], Vec::new()),
            owned(vec![item("two")], Vec::new()),
            owned(vec![item_with_payload("one", text("one-final"))], Vec::new()),
        ]);
        for now in 1..=3 {
            pump.push_stdout_chunk_deferred(b"{}\n", now)
                .await
                .unwrap();
        }
        assert!(shared.0.lock().unwrap().log.is_empty());
        assert!(pump.flush_deferred().await.unwrap());
        let state = shared.0.lock().unwrap();
        assert_eq!(state.log, ["journal", "publish"]);
        assert_eq!(state.durable_batches.len(), 1);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [
                DurableAdapterEvent::Item { external_item_id: one, payload: NormalizedPayload::Text { text }, .. },
                DurableAdapterEvent::Item { external_item_id: two, .. }
            ] if one.as_str() == "one" && text.as_str() == "one-final" && two.as_str() == "two"
        ));
    });
}

#[test]
fn cursor_tool_lifecycle_coalesces_into_one_item_under_one_external_id() {
    runtime().block_on(async {
        let external = "call-98fc79df-e1f3-4bef-8df7-b01cec8e0fa4-0\nfc_2f34589c-2f13-9b7d-a509-58a49716544b_0";
        let started = AdapterEvent::Item {
            id: identity(external),
            lifecycle: ItemLifecycle::Started,
            tool_state: Some(ToolState::Pending),
            payload: NormalizedPayload::ToolCall {
                execution: Default::default(),
                category: ToolCategory::Read,
                label: SafeText::new("label", "read").unwrap(),
                input_summary: None,
                output_summary: None,
                locations: Default::default(),
                command_activity: Default::default(),
            },
        };
        let progressed = |lifecycle, tool_state| AdapterEvent::Item {
            id: identity(external),
            lifecycle,
            tool_state: Some(tool_state),
            payload: NormalizedPayload::ToolCall {
                execution: Default::default(),
                category: ToolCategory::Read,
                label: SafeText::new("label", "read").unwrap(),
                input_summary: None,
                output_summary: None,
                locations: Default::default(),
                command_activity: Default::default(),
            },
        };
        let (mut pump, shared) = build_pump(vec![
            owned(vec![started], Vec::new()),
            owned(
                vec![progressed(ItemLifecycle::Streaming, ToolState::InProgress)],
                Vec::new(),
            ),
            owned(
                vec![progressed(ItemLifecycle::Completed, ToolState::Completed)],
                Vec::new(),
            ),
        ]);
        for now in 1..=3 {
            pump.push_stdout_chunk_deferred(b"{}\n", now).await.unwrap();
        }
        assert!(pump.flush_deferred().await.unwrap());
        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), 1);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::Item {
                external_item_id,
                lifecycle: ItemLifecycle::Completed,
                tool_state: Some(ToolState::Completed),
                payload: NormalizedPayload::ToolCall { .. },
                ..
            }] if external_item_id.as_str() == external
        ));
    });
}

#[test]
fn admitted_file_diff_items_stay_adjacent_to_their_pair() {
    let mut pending = PendingIngress::new();
    pending
        .push(vec![normalized_fact("diff", admitted_diff())], 1, 0)
        .unwrap();
    pending.push(vec![file_pair_fact("diff")], 1, 1).unwrap();
    pending
        .push(vec![normalized_fact("diff", text("late"))], 1, 2)
        .unwrap();
    assert!(matches!(
        pending.take().unwrap().0.as_slice(),
        [
            DurableAdapterEvent::Item {
                payload: NormalizedPayload::FileChangeDiff { .. },
                ..
            },
            DurableAdapterEvent::FilePair { .. },
            DurableAdapterEvent::Item {
                payload: NormalizedPayload::Text { .. },
                ..
            }
        ]
    ));
}

#[test]
fn native_child_identity_survives_a_later_generic_item_update_in_one_window() {
    let mut pending = PendingIngress::new();
    pending
        .push(vec![child_thread_fact("activity", "child-one")], 1, 0)
        .unwrap();
    pending
        .push(
            vec![normalized_fact("activity", collaboration_payload())],
            1,
            1,
        )
        .unwrap();
    assert!(matches!(pending.take().unwrap().0.as_slice(),
        [DurableAdapterEvent::ChildThreadItem { child_thread_id, .. },
         DurableAdapterEvent::Item { external_item_id, .. }]
            if child_thread_id.as_str() == "child-one" && external_item_id.as_str() == "activity"
    ));
}

#[test]
fn native_child_identity_facts_fail_closed_at_batch_capacity() {
    let mut pending = PendingIngress::new();
    for index in 0..LIMITS.queues.journal_facts_per_transaction {
        pending
            .push(
                vec![child_thread_fact(
                    &format!("activity-{index}"),
                    &format!("child-{index}"),
                )],
                1,
                index as i64,
            )
            .unwrap();
    }
    assert!(matches!(
        pending.push(
            vec![child_thread_fact("activity-over", "child-over")],
            1,
            129
        ),
        Err(AdapterPumpError::BatchCapacity)
    ));
    assert_eq!(
        pending.take().unwrap().0.len(),
        LIMITS.queues.journal_facts_per_transaction as usize
    );
}

#[test]
fn deferred_usage_snapshots_coalesce_on_one_turn_scoped_identity() {
    runtime().block_on(async {
        let usage = |input_tokens, context_window| AdapterEvent::Usage {
            turn_id: identity("external-turn"),
            payload: NormalizedPayload::Usage {
                input_tokens,
                output_tokens: 2,
                context_window,
            },
        };
        let (mut pump, shared) = build_pump(vec![
            owned(vec![usage(3, Some(272_000))], Vec::new()),
            owned(vec![usage(8, None)], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n", 2).await.unwrap();
        assert!(pump.flush_deferred().await.unwrap());

        let state = shared.0.lock().unwrap();
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::UsageSnapshot {
                input_tokens: 8,
                output_tokens: 2,
                context_window: None,
                ..
            }]
        ));
    });
}

#[test]
fn deferred_deadline_and_terminal_flush_without_polling_or_reordering() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Running), item("one")], Vec::new()),
            owned(
                vec![
                    terminal(TurnState::WaitingTool),
                    item_with_payload("one", text("after-wait")),
                ],
                Vec::new(),
            ),
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 100).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n", 101).await.unwrap();
        assert_eq!(pump.next_deferred_flush_deadline_millis(), Some(150));
        assert!(!pump.flush_deferred_if_needed(149).await.unwrap());
        assert!(pump.flush_deferred_if_needed(150).await.unwrap());
        pump.push_stdout_chunk_deferred(b"{}\n", 151).await.unwrap();
        assert!(pump.completed_turn_ready());
        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), 2);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [
                DurableAdapterEvent::Turn { state: TurnState::Running, .. },
                DurableAdapterEvent::Item { .. },
                DurableAdapterEvent::Turn { state: TurnState::WaitingTool, .. },
                DurableAdapterEvent::Item { payload: NormalizedPayload::Text { text }, .. }
            ] if text.as_str() == "after-wait"
        ));
    });
}

#[test]
fn pending_actual_bytes_are_exact_and_one_identity_coalesces_across_kinds() {
    let maximum = LIMITS.queues.journal_bytes_per_transaction as usize;
    let fact = normalized_fact("same", text("one"));
    let mut exact = PendingIngress::new();
    assert!(!exact.would_exceed(std::slice::from_ref(&fact), maximum));
    exact.push(vec![fact], maximum, 0).unwrap();
    assert_eq!(exact.take().unwrap().1, maximum);

    let plus_one = normalized_fact("same", text("one"));
    assert!(PendingIngress::new().would_exceed(std::slice::from_ref(&plus_one), maximum + 1));

    let mut identities = PendingIngress::new();
    identities
        .push(vec![normalized_fact("same", text("text"))], 1, 0)
        .unwrap();
    identities
        .push(
            vec![normalized_fact(
                "same",
                NormalizedPayload::ToolCall {
                    execution: Default::default(),
                    category: ToolCategory::Other,
                    label: SafeText::new("label", "tool").unwrap(),
                    input_summary: None,
                    output_summary: None,
                    locations: Default::default(),
                    command_activity: Default::default(),
                },
            )],
            1,
            1,
        )
        .unwrap();
    assert_eq!(identities.take().unwrap().0.len(), 1);
}

#[test]
fn completed_turn_handoff_rebinds_without_dropping_session_authority() {
    runtime().block_on(async {
        let (mut pump, _) = build_pump(vec![owned(
            vec![terminal(TurnState::Completed)],
            Vec::new(),
        )]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        parts.journal.allow_rebind = false;
        let (mut parts, error) = *parts
            .bind_turn(next_turn())
            .err()
            .expect("rebind refusal retains parts");
        assert!(matches!(
            error,
            AdapterPumpError::Journal(PumpJournalError::Rejected)
        ));
        parts.journal.allow_rebind = true;
        let next = parts.bind_turn(next_turn()).ok().unwrap();
        assert!(!next.teardown_required());
    });
}

#[test]
fn completed_turn_handoff_keeps_synthetic_item_identity_unique() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![unsupported()], Vec::new()),
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(vec![unsupported()], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n", 2).await.unwrap();
        let parts = pump.try_into_completed_parts().ok().unwrap();
        let mut next = parts.bind_turn(next_turn()).ok().unwrap();
        next.push_stdout_chunk_deferred(b"{}\n", 3).await.unwrap();

        let state = shared.0.lock().unwrap();
        let identities = state
            .durable_batches
            .iter()
            .flat_map(|batch| batch.iter())
            .filter_map(|event| match event {
                DurableAdapterEvent::RuntimePayload {
                    external_item_id, ..
                } => Some(external_item_id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(identities, ["sage-runtime-1-1", "sage-runtime-1-2"]);
    });
}

#[test]
fn stdout_preamble_is_skipped_only_before_the_first_frame_and_stays_fatal_after_it() {
    runtime().block_on(async {
        let (mut pump, _shared) = build_pump(vec![owned(vec![item("item-one")], Vec::new())]);
        assert!(matches!(
            pump.push_stdout_chunk(b"Loaded cached credentials.\n", 10)
                .await,
            Ok(false)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 11).await,
            Ok(true)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(b"Loaded cached credentials.\n", 12)
                .await,
            Err(AdapterPumpError::Frame)
        ));
    });
}

#[test]
fn stdout_preamble_stops_being_tolerated_past_its_line_bound() {
    runtime().block_on(async {
        let (mut pump, _shared) = build_pump(vec![owned(vec![item("item-one")], Vec::new())]);
        for line in 0..STDOUT_PREAMBLE_MAX_LINES {
            assert!(matches!(
                pump.push_stdout_chunk(b"Loaded cached credentials.\n", i64::from(line))
                    .await,
                Ok(false)
            ));
        }
        assert!(matches!(
            pump.push_stdout_chunk(b"Loaded cached credentials.\n", 100)
                .await,
            Err(AdapterPumpError::Frame)
        ));
    });
}

#[test]
fn malformed_json_is_never_mistaken_for_stdout_preamble() {
    runtime().block_on(async {
        for malformed in [
            b"{]\n".as_slice(),
            b"[}\n".as_slice(),
            b"  {oops}\n".as_slice(),
        ] {
            let (mut pump, _shared) = build_pump(Vec::new());
            assert!(matches!(
                pump.push_stdout_chunk(malformed, 10).await,
                Err(AdapterPumpError::Frame)
            ));
        }
    });
}

#[test]
fn stdout_preamble_stops_being_tolerated_past_its_byte_bound() {
    runtime().block_on(async {
        let (mut pump, _shared) = build_pump(vec![owned(vec![item("item-one")], Vec::new())]);
        let mut line = "x".repeat(STDOUT_PREAMBLE_MAX_BYTES / 2 + 1);
        line.push('\n');
        assert!(matches!(
            pump.push_stdout_chunk(line.as_bytes(), 10).await,
            Ok(false)
        ));
        assert!(matches!(
            pump.push_stdout_chunk(line.as_bytes(), 11).await,
            Err(AdapterPumpError::Frame)
        ));
    });
}

fn build_pump(
    batches: Vec<AdapterBatch>,
) -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    pump_with_queue(
        batches,
        queue_with(
            LIMITS.queues.egress_frames as usize,
            LIMITS.queues.egress_bytes as usize,
            LIMITS.queues.egress_control_frames_inside_total as usize,
            LIMITS.queues.egress_control_bytes_inside_total as usize,
        ),
    )
}

fn permission_pump() -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    let event = AdapterEvent::PermissionRequest {
        turn_id: identity("turn-envelope"),
        item_id: identity("external-request"),
        external_request_id: ExternalId::new("permissionRequest", "external-request").unwrap(),
        payload: permission(),
        request_payload_digest: [1; 32],
    };
    build_pump(vec![owned(vec![event], Vec::new())])
}

fn permission_request_id(shared: &Shared) -> SageServerRequestId {
    match &shared.0.lock().unwrap().durable_batches[0][0] {
        DurableAdapterEvent::PermissionRequest { intent, .. } => intent.request_id.clone(),
        _ => panic!("permission request must be durable before resolution"),
    }
}

fn pump_with_queue(
    batches: Vec<AdapterBatch>,
    queue: ReservedQueue<AdapterEgress>,
) -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    pump_configured(batches, VecDeque::new(), VecDeque::new(), queue)
}

fn pump_configured(
    batches: Vec<AdapterBatch>,
    journal_outcomes: VecDeque<Result<ProjectionRevision, PumpJournalError>>,
    publisher_outcomes: VecDeque<Result<ProjectionPublishStatus, ProjectionPublishError>>,
    queue: ReservedQueue<AdapterEgress>,
) -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    pump_configured_for_scope(
        batches,
        journal_outcomes,
        publisher_outcomes,
        queue,
        session_intent(),
    )
}

fn pump_configured_for_scope(
    batches: Vec<AdapterBatch>,
    journal_outcomes: VecDeque<Result<ProjectionRevision, PumpJournalError>>,
    publisher_outcomes: VecDeque<Result<ProjectionPublishStatus, ProjectionPublishError>>,
    queue: ReservedQueue<AdapterEgress>,
    session_scope: SessionPumpIntent,
) -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    let shared = Shared(Arc::new(Mutex::new(FakeState {
        batches: batches.into(),
        ..FakeState::default()
    })));
    let publisher: Arc<dyn ProjectionPublisher> = Arc::new(FakePublisher {
        shared: shared.clone(),
        outcomes: Mutex::new(publisher_outcomes),
    });
    let pump = AdapterPump::with_queue(
        FakeAdapter {
            shared: shared.clone(),
            next_request: 0,
        },
        FakeJournal {
            shared: shared.clone(),
            outcomes: journal_outcomes,
            allow_rebind: true,
            next_projection: 0,
            turn_id: turn("one"),
            turn_state: TurnState::Sending,
            turn_revision: 1,
        },
        Mapper,
        publisher,
        session_scope.bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: Some(ExternalId::new("turn", "external-turn").unwrap()),
            client_message_id: message(),
        }),
        queue,
    )
    .unwrap();
    (pump, shared)
}

fn owned(events: Vec<AdapterEvent>, egress: Vec<AdapterEgress>) -> AdapterBatch {
    AdapterBatch::new(events, egress).unwrap()
}

fn item(external: &str) -> AdapterEvent {
    item_with_payload(external, text(external))
}

fn item_with_payload(external: &str, payload: NormalizedPayload) -> AdapterEvent {
    AdapterEvent::Item {
        id: identity(external),
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload,
    }
}

fn activity_item(parent_item_id: AdapterIdentity) -> AdapterEvent {
    AdapterEvent::ActivityItem(Box::new(
        crate::acp::bindings::adapter::AdapterActivityItem {
            activity_id: identity("child-activity"),
            item_id: identity("child-item"),
            parent_activity_id: None,
            parent_item_id,
            activity_kind: RequiredText::new("activityKind", "nativeChild").unwrap(),
            role: ActivityRole::Child,
            activity_lifecycle: ActivityLifecycle::Observed,
            depth: 1,
            observed_sequence: 1,
            provenance: ActivityProvenanceV1 {
                schema_version: SchemaVersionV1,
                source_kind: ActivitySourceKind::Collaboration,
                external_kind: RequiredText::new("externalKind", "nativeChild").unwrap(),
                extension_schema_id: None,
                safe_label: None,
            },
            item_lifecycle: ItemLifecycle::Completed,
            item_tool_state: None,
            payload: text("child text"),
        },
    ))
}

fn normalized_fact(external: &str, payload: NormalizedPayload) -> NormalizedFact {
    NormalizedFact::new(
        DurableAdapterEvent::Item {
            scope: fact_scope(),
            external_item_id: ExternalId::new("item", external).unwrap(),
            lifecycle: ItemLifecycle::Streaming,
            tool_state: None,
            payload,
            payload_digest: "digest".into(),
            now_millis: 0,
        },
        1,
    )
}

fn collaboration_payload() -> NormalizedPayload {
    NormalizedPayload::Collaboration {
        label: SafeText::new("label", "Worker").unwrap(),
        message: None,
        receiver_labels: Default::default(),
        operation: Some(SafeText::new("operation", "started").unwrap()),
        agent_key: None,
        agent_keys: Default::default(),
        agent_states: Default::default(),
    }
}

fn child_thread_fact(external: &str, child: &str) -> NormalizedFact {
    NormalizedFact::new(
        DurableAdapterEvent::ChildThreadItem {
            scope: fact_scope(),
            external_item_id: ExternalId::new("item", external).unwrap(),
            lifecycle: ItemLifecycle::Started,
            payload: collaboration_payload(),
            payload_digest: "digest".into(),
            child_thread_id: ExternalId::new("child", child).unwrap(),
            now_millis: 0,
        },
        1,
    )
}

fn fact_scope() -> TurnScope {
    TurnScope {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: session("one"),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: turn("one"),
        binding_digest: "07".repeat(32),
    }
}

fn file_pair_fact(external: &str) -> NormalizedFact {
    NormalizedFact::new(
        DurableAdapterEvent::FilePair {
            scope: fact_scope(),
            external_item_id: ExternalId::new("item", external).unwrap(),
            old_text: "old".into(),
            new_text: "new".into(),
            now_millis: 0,
        },
        1,
    )
}

fn admitted_diff() -> NormalizedPayload {
    NormalizedPayload::file_admitted(
        crate::acp::intent::FileChangeFacts {
            resource: SafeResourceLocation::unavailable(UnavailableResourceReason::Missing),
            summary: SafeText::new("summary", "lib.rs").unwrap(),
            additions: Some(1),
            deletions: Some(1),
            operation: crate::acp::intent::FileChangeOperation::Updated,
        },
        1,
        (3, 3),
        "0a".repeat(32),
    )
}

fn next_turn() -> ActiveTurnIntent {
    ActiveTurnIntent {
        turn_id: turn("two"),
        external_turn_id: Some(ExternalId::new("turn", "external-turn-two").unwrap()),
        client_message_id: RequiredText::new("message", "message-two").unwrap(),
    }
}

fn terminal(state: TurnState) -> AdapterEvent {
    AdapterEvent::Turn {
        id: identity("turn-envelope"),
        state,
    }
}

fn identity(external: &str) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        session_id: session("one"),
        external_id: ExternalId::new("external", external).unwrap(),
        turn_id: Some(ExternalId::new("turn", "external-turn").unwrap()),
        turn_client_message_id: Some(message()),
    }
}

fn text(value: &str) -> NormalizedPayload {
    NormalizedPayload::Text {
        text: SafeText::new("text", value).unwrap(),
    }
}

fn unsupported() -> AdapterEvent {
    AdapterEvent::Unsupported(NormalizedPayload::Unsupported {
        external_kind: SafeText::new("externalKind", "thread/status/changed").unwrap(),
        summary: SafeText::new("summary", "Unsupported Codex event").unwrap(),
    })
}

fn permission() -> NormalizedPayload {
    NormalizedPayload::PermissionRequest {
        category: ToolCategory::Other,
        summary: SafeText::new("summary", "denied by policy").unwrap(),
        offered_option_kinds: None,
    }
}

fn session_intent() -> SessionPumpIntent {
    SessionPumpIntent {
        binding_digest: [7; 32],
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: session("one"),
        external_session_id: ExternalId::new("session", "external-session").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
}

fn session(value: &str) -> SageAgentSessionId {
    SageAgentSessionId::parse(format!("session-{value}")).unwrap()
}

fn turn(value: &str) -> SageTurnId {
    SageTurnId::parse(format!("turn-{value}")).unwrap()
}

fn message() -> RequiredText<512> {
    RequiredText::new("message", "message-one").unwrap()
}

fn revision(value: u64) -> ProjectionRevision {
    ProjectionRevision::new(value).unwrap()
}

fn queue_with(
    total_count: usize,
    total_bytes: usize,
    control_count: usize,
    control_bytes: usize,
) -> ReservedQueue<AdapterEgress> {
    ReservedQueue::new(total_count, total_bytes, control_count, control_bytes).unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[path = "cancel_tests.rs"]
mod cancel_tests;

#[path = "child_binding_tests.rs"]
mod child_binding_tests;

#[path = "child_store_tests.rs"]
mod child_store_tests;

#[path = "completion_tests.rs"]
mod completion_tests;

#[path = "idle_egress_tests.rs"]
mod idle_egress_tests;

#[path = "sign_in_tests.rs"]
mod sign_in_tests;

#[path = "terminal_error_tests.rs"]
mod terminal_error_tests;
