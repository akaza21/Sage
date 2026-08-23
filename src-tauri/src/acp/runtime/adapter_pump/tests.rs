use std::{collections::VecDeque, future::Future, pin::Pin, sync::Mutex};

use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterIdentity, EgressToken},
    contract::{EntityRevision, SafeText, SchemaVersionV1},
    intent::{
        ActivityProvenanceV1, ActivitySourceKind, CapabilitySnapshotV1, SafeResourceRef,
        ToolCategory,
    },
    lifecycle::{ActivityLifecycle, DeliveryState, ServerRequestState},
    projection::ActivityRole,
    runtime::ProjectionPublishError,
};

use super::coalescer::{NormalizedFact, PendingIngress};
use super::*;

#[derive(Default)]
struct FakeState {
    batches: VecDeque<AdapterBatch>,
    commands: Vec<&'static str>,
    resolutions: Vec<EgressResolution>,
    durable_batches: Vec<Vec<DurableAdapterEvent>>,
    request_states: Vec<ServerRequestState>,
    permission_failure: Option<(ServerRequestState, PumpJournalError)>,
    log: Vec<&'static str>,
    ingest_error: Option<AdapterRuntimeError>,
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

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let AdapterCommand::ResolvePermission { decision, .. } = command {
            let request_id = ExternalId::new("request", "external-request")
                .map_err(|_| AdapterRuntimeError::Frame)?;
            let token = match decision {
                crate::acp::runtime::permissions::PermissionDecision::Ask => {
                    EgressToken::protocol_notification()
                }
                crate::acp::runtime::permissions::PermissionDecision::AllowOnce => {
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
                    && intent.request_id.as_str() == "request-external"
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

        let state = shared.0.lock().unwrap();
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::PermissionRequest { .. }]
        ));
        assert_eq!(state.log, ["journal", "publish"]);
        assert!(state.resolutions.is_empty());
        drop(state);
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
        assert_eq!(
            queue_failure.flush_one(&mut queued).unwrap(),
            FlushDisposition::Written
        );
        assert_eq!(queued.0, 1);
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
        let state = shared.0.lock().unwrap();
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::PermissionRequest { .. }]
        ));
        assert_eq!(state.log, ["journal", "publish"]);
        assert!(state.resolutions.is_empty());
        drop(state);
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
        pump.queue_cancel(
            &session("foreign"),
            RuntimeEpoch::new(1).unwrap(),
            &turn("one"),
            &message()
        ),
        Err(AdapterPumpError::NoActiveTurn)
    ));
    assert!(shared.0.lock().unwrap().commands.is_empty());
    assert_eq!(
        pump.queue_cancel(
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
        pump.queue_cancel(
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
            pump.queue_cancel(
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
            Err(AdapterPumpError::TeardownRequired)
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
fn pending_actual_bytes_are_exact_and_kind_conflicts_are_not_hidden() {
    let maximum = LIMITS.queues.journal_bytes_per_transaction as usize;
    let fact = normalized_fact("same", text("one"));
    let mut exact = PendingIngress::new();
    assert!(!exact.would_exceed(std::slice::from_ref(&fact), maximum));
    exact.push(vec![fact], maximum, 0).unwrap();
    assert_eq!(exact.take().unwrap().1, maximum);

    let plus_one = normalized_fact("same", text("one"));
    assert!(PendingIngress::new().would_exceed(std::slice::from_ref(&plus_one), maximum + 1));

    let mut kinds = PendingIngress::new();
    kinds
        .push(vec![normalized_fact("same", text("text"))], 1, 0)
        .unwrap();
    kinds
        .push(
            vec![normalized_fact(
                "same",
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Other,
                    label: SafeText::new("label", "tool").unwrap(),
                    input_summary: None,
                },
            )],
            1,
            1,
        )
        .unwrap();
    assert_eq!(kinds.take().unwrap().0.len(), 2);
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
        session_intent().bind_turn(ActiveTurnIntent {
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
            scope: TurnScope {
                project_id: SageProjectId::parse("project-one").unwrap(),
                chat_id: SageChatId::parse("chat-one").unwrap(),
                session_id: session("one"),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                turn_id: turn("one"),
                binding_digest: "07".repeat(32),
            },
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

fn permission() -> NormalizedPayload {
    NormalizedPayload::PermissionRequest {
        category: ToolCategory::Other,
        summary: SafeText::new("summary", "denied by policy").unwrap(),
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
