use serde_json::{Value, json};

use crate::acp::{
    contract::{
        ActivityLifecycle, BoundedVec, ClientIntentId, EntityRevision, ItemLifecycle,
        ProjectGeneration, ProjectionRevision, RequiredText, Residency, RuntimeEpoch,
        RuntimeEpochState, SafeText, SageAgentSessionId, SageChatId, SageItemId, SageProjectId,
        SageRootId, SageTurnId, SchemaVersionV1, ServerRequestState, SessionState, TerminalUpdate,
        ToolState, TurnState, classify_terminal,
    },
    intent::{
        AcpIpcError, ActivateAcpSessionRequest, ActivityProvenanceV1, CancelAcpTurnRequest,
        CapabilitySnapshotV1, CloseAcpSessionRequest, GetAcpChatPageRequest, IntentError,
        IntentSnapshot, NormalizedPayload, ProjectionChangedV1, PromptDispatch, SafeResourceRef,
        SendAcpTurnFailure, SendAcpTurnRequest, StartAcpSessionRequest, SteerAcpTurnRequest,
        validate_base_intent, validate_cancel_intent, validate_session_intent,
        validate_steer_intent,
    },
    limits::LIMITS,
    projection::{
        AcpChatPage, ItemContent, ItemOrigin, ItemProjection, SessionProjection, TextRole,
        TurnProjection,
    },
};

fn request_base() -> Value {
    json!({
        "projectId": "project-a",
        "projectGeneration": "3",
        "chatId": "chat-a",
        "expectedProjectionRevision": "7",
        "clientIntentId": "intent-a"
    })
}

fn with(mut value: Value, fields: Value) -> Value {
    value
        .as_object_mut()
        .expect("request is an object")
        .extend(fields.as_object().expect("fields are an object").clone());
    value
}

fn ids() -> (SageProjectId, SageChatId, SageAgentSessionId, SageTurnId) {
    (
        SageProjectId::parse("project-a").unwrap(),
        SageChatId::parse("chat-a").unwrap(),
        SageAgentSessionId::parse("session-a").unwrap(),
        SageTurnId::parse("turn-a").unwrap(),
    )
}

fn snapshot() -> IntentSnapshot {
    let (project_id, chat_id, session_id, turn_id) = ids();
    IntentSnapshot {
        project_id,
        project_generation: ProjectGeneration::new(3).unwrap(),
        chat_id,
        projection_revision: ProjectionRevision::new(7).unwrap(),
        session_id: Some(session_id),
        runtime_epoch: Some(RuntimeEpoch::new(2).unwrap()),
        turn_id: Some(turn_id),
        turn_state: Some(TurnState::Running),
        turn_revision: Some(EntityRevision::new(5).unwrap()),
    }
}

#[test]
fn ids_text_and_counters_are_bounded_and_canonical() {
    assert!(SageChatId::new().as_str().starts_with("chat-"));
    assert!(ClientIntentId::new().as_str().starts_with("intent-"));
    assert!(SageProjectId::parse("").is_err());
    assert!(SageProjectId::parse("a".repeat(513)).is_err());
    assert!(SafeText::<4>::new("value", "five!").is_err());
    assert!(SafeText::<8>::new("value", "a\0b").is_err());
    assert_eq!(
        serde_json::to_string(&ProjectGeneration::ZERO).unwrap(),
        "\"0\""
    );
    assert!(serde_json::from_str::<ProjectGeneration>("\"01\"").is_err());
    assert!(serde_json::from_str::<ProjectGeneration>("1").is_err());
    assert!(
        ProjectGeneration::new(i64::MAX as u64)
            .unwrap()
            .next()
            .is_err()
    );
}

#[test]
fn lifecycle_tables_reject_reopen_and_implement_first_terminal_wins() {
    assert!(SessionState::Created.can_transition_to(SessionState::Starting));
    assert!(SessionState::Recovering.can_transition_to(SessionState::Incompatible));
    assert!(!SessionState::Closed.can_transition_to(SessionState::Ready));
    assert!(SessionState::Incompatible.is_terminal());

    assert!(RuntimeEpochState::Starting.can_transition_to(RuntimeEpochState::Running));
    assert!(RuntimeEpochState::Starting.can_transition_to(RuntimeEpochState::Aborted));
    assert!(!RuntimeEpochState::Exited.can_transition_to(RuntimeEpochState::Running));
    assert!(RuntimeEpochState::Legacy.is_terminal());
    assert!(RuntimeEpochState::Aborted.is_terminal());

    assert!(TurnState::Queued.can_transition_to(TurnState::Sending));
    assert!(!TurnState::Queued.can_transition_to(TurnState::Failed));
    assert!(TurnState::Sending.can_transition_to(TurnState::Cancelling));
    assert!(TurnState::Sending.can_transition_to(TurnState::DeliveryUnknown));
    for waiting in [
        TurnState::WaitingModel,
        TurnState::WaitingTool,
        TurnState::WaitingApproval,
        TurnState::WaitingUser,
    ] {
        assert!(TurnState::Running.can_transition_to(waiting));
        assert!(waiting.can_transition_to(TurnState::Running));
        assert!(waiting.can_transition_to(TurnState::Cancelling));
        assert!(waiting.can_transition_to(TurnState::Completed));
        for other_waiting in [
            TurnState::WaitingModel,
            TurnState::WaitingTool,
            TurnState::WaitingApproval,
            TurnState::WaitingUser,
        ] {
            assert!(!waiting.can_transition_to(other_waiting));
        }
    }
    assert!(TurnState::Cancelling.can_transition_to(TurnState::Interrupted));
    assert!(TurnState::Cancelling.can_transition_to(TurnState::DeliveryUnknown));
    assert!(!TurnState::Completed.can_transition_to(TurnState::Running));
    assert!(TurnState::DeliveryUnknown.is_terminal());

    assert!(ItemLifecycle::Started.can_transition_to(ItemLifecycle::Streaming));
    assert!(ItemLifecycle::Streaming.can_transition_to(ItemLifecycle::Unsupported));
    assert!(!ItemLifecycle::Completed.can_transition_to(ItemLifecycle::Streaming));
    assert!(ItemLifecycle::Cancelled.is_terminal());

    assert!(ToolState::Pending.can_transition_to(ToolState::WaitingForApproval));
    assert!(ToolState::WaitingForApproval.can_transition_to(ToolState::Declined));
    assert!(!ToolState::Completed.can_transition_to(ToolState::Failed));
    assert!(ToolState::Declined.is_terminal());

    assert!(ServerRequestState::Received.can_transition_to(ServerRequestState::DeclinePending));
    assert!(!ServerRequestState::DeclineCommitted.can_transition_to(ServerRequestState::Received));
    assert!(ServerRequestState::ResolvedDeclined.is_terminal());
    assert!(ActivityLifecycle::Observed.can_transition_to(ActivityLifecycle::Active));
    assert!(!ActivityLifecycle::Completed.can_transition_to(ActivityLifecycle::Active));

    assert_eq!(classify_terminal::<u8>(None, &1), TerminalUpdate::Apply);
    assert_eq!(classify_terminal(Some(&1), &1), TerminalUpdate::NoOp);
    assert_eq!(classify_terminal(Some(&1), &2), TerminalUpdate::Conflict);
}

#[test]
fn protocol_contract_turn_transitions_match_the_rust_lifecycle() {
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/protocol-contract-v1.json"
    ))
    .unwrap();
    let declared = contract["turnTransitions"].as_object().unwrap();
    let states = [
        ("queued", TurnState::Queued),
        ("sending", TurnState::Sending),
        ("running", TurnState::Running),
        ("waitingModel", TurnState::WaitingModel),
        ("waitingTool", TurnState::WaitingTool),
        ("waitingApproval", TurnState::WaitingApproval),
        ("waitingUser", TurnState::WaitingUser),
        ("cancelling", TurnState::Cancelling),
        ("completed", TurnState::Completed),
        ("interrupted", TurnState::Interrupted),
        ("cancelled", TurnState::Cancelled),
        ("failed", TurnState::Failed),
        ("deliveryUnknown", TurnState::DeliveryUnknown),
    ];
    assert_eq!(declared.len(), states.len());
    for (source_name, source) in states {
        let targets = declared[source_name].as_array().unwrap();
        for (target_name, target) in states {
            let listed = targets
                .iter()
                .any(|value| value.as_str() == Some(target_name));
            assert_eq!(
                listed,
                source.can_transition_to(target),
                "protocol transition drift for {source_name} -> {target_name}"
            );
        }
    }
}

#[test]
fn protocol_contract_item_kinds_match_the_rust_wire_set() {
    use crate::acp::contract::ItemKind;
    let contract: Value = serde_json::from_str(include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/protocol-contract-v1.json"
    ))
    .unwrap();
    let kinds = [
        ItemKind::Text,
        ItemKind::ThoughtSummary,
        ItemKind::Plan,
        ItemKind::ToolCall,
        ItemKind::FileChangeDiff,
        ItemKind::Image,
        ItemKind::Usage,
        ItemKind::PermissionRequest,
        ItemKind::Error,
        ItemKind::Unsupported,
        ItemKind::Collaboration,
    ];
    for kind in kinds {
        match kind {
            ItemKind::Text
            | ItemKind::ThoughtSummary
            | ItemKind::Plan
            | ItemKind::ToolCall
            | ItemKind::FileChangeDiff
            | ItemKind::Image
            | ItemKind::Usage
            | ItemKind::PermissionRequest
            | ItemKind::Error
            | ItemKind::Unsupported
            | ItemKind::Collaboration => {}
        }
    }
    let declared = contract["itemKinds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(declared, kinds.map(ItemKind::as_str).to_vec());
}

#[test]
fn sqlite_rejects_every_cross_wait_turn_transition() {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection
        .execute_batch(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at)
               VALUES('project-a','Project',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
               VALUES('chat-a','project-a',0,'Chat',1,1);
             INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,
               capabilities_json,state,residency,created_at,updated_at
             ) VALUES(
               'session-a','project-a','chat-a',0,'binding-a',
               'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
               '{}','active','hot',1,1
             );
             INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,started_at
             ) VALUES('project-a','chat-a','session-a',1,'running',1);
             INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,
               client_intent_id,state,delivery_state,created_at,updated_at
             ) VALUES(
               'turn-a','project-a','chat-a','session-a',1,0,
               'intent-a','running','correlated',1,1
             );",
        )
        .unwrap();

    let waiting_states = [
        "waitingModel",
        "waitingTool",
        "waitingApproval",
        "waitingUser",
    ];
    for source in waiting_states {
        connection
            .execute("UPDATE turns SET state=?1 WHERE id='turn-a'", [source])
            .unwrap();
        for target in waiting_states {
            if target != source {
                assert!(
                    connection
                        .execute("UPDATE turns SET state=?1 WHERE id='turn-a'", [target])
                        .is_err(),
                    "{source} must not transition directly to {target}"
                );
            }
        }
        connection
            .execute("UPDATE turns SET state='running' WHERE id='turn-a'", [])
            .unwrap();
    }
}

#[test]
fn production_schema_exactly_matches_the_reviewed_schema_contract() {
    let production = include_str!("../../../migrations/005_acp_session_foundation.sql");
    let contract = include_str!(
        "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/schema-005.sql"
    );
    assert_eq!(production.as_bytes(), contract.as_bytes());
}

#[test]
fn normalized_payloads_admit_only_canonical_resource_refs() {
    let root_id = SageRootId::parse("root-a").unwrap();
    let resource = SafeResourceRef::new(root_id, "src/main.rs").unwrap();
    let payload = NormalizedPayload::file_summary_only(
        crate::acp::intent::FileChangeFacts {
            resource: crate::acp::intent::SafeResourceLocation::Resource { resource },
            summary: SafeText::new("summary", "one file changed").unwrap(),
            additions: Some(2),
            deletions: Some(1),
            operation: crate::acp::intent::FileChangeOperation::Updated,
        },
        false,
    );
    assert_eq!(
        payload.kind(),
        crate::acp::contract::ItemKind::FileChangeDiff
    );
    assert!(SafeResourceRef::new(SageRootId::parse("root-a").unwrap(), "/tmp/x").is_err());
    assert!(SafeResourceRef::new(SageRootId::parse("root-a").unwrap(), "C:/tmp/x").is_err());
    assert!(SafeResourceRef::new(SageRootId::parse("root-a").unwrap(), "a/../b").is_err());
    assert!(
        serde_json::from_value::<SafeResourceRef>(json!({
            "rootId": "root-a", "relativePath": "src/main.rs", "absolutePath": "/tmp/x"
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<NormalizedPayload>(json!({
            "kind": "text", "payload": {"text": "safe", "rawFrame": "secret"}
        }))
        .is_err()
    );

    let exact = "a".repeat(LIMITS.wire.relative_path_bytes_native_only as usize);
    assert!(SafeResourceRef::new(SageRootId::parse("root-a").unwrap(), exact.clone()).is_ok());
    assert!(
        SafeResourceRef::new(SageRootId::parse("root-a").unwrap(), format!("{exact}a")).is_err()
    );
}

#[test]
fn image_payloads_use_the_image_kind_and_reject_unknown_media_types() {
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";
    let payload =
        crate::acp::image_admit::admit_image_data_url(&format!("data:image/png;base64,{PNG}"))
            .unwrap();
    assert_eq!(payload.kind(), crate::acp::contract::ItemKind::Image);
    assert_eq!(payload.kind().as_str(), "image");
    let legacy = serde_json::from_value::<NormalizedPayload>(json!({
        "kind": "image",
        "payload": { "mediaType": "image/png", "data": PNG }
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_value(legacy).unwrap()["payload"]["source"],
        "agent"
    );
    let user_image = ItemContent::Image {
        media_type: crate::acp::intent::ImageMediaType::Png,
        data_url: SafeText::new("dataUrl", format!("data:image/png;base64,{PNG}")).unwrap(),
        source: crate::acp::intent::ImageSource::User,
    };
    assert_eq!(
        serde_json::to_value(user_image).unwrap()["payload"]["source"],
        "user"
    );
    assert!(
        serde_json::from_value::<NormalizedPayload>(json!({
            "kind": "image",
            "payload": { "mediaType": "image/svg+xml", "data": "AAAA" }
        }))
        .is_err()
    );
}

#[test]
fn bounded_vectors_reject_limit_plus_one_without_retaining_it() {
    assert!(BoundedVec::<u8, 2>::new("values", vec![1, 2]).is_ok());
    assert!(BoundedVec::<u8, 2>::new("values", vec![1, 2, 3]).is_err());
    assert!(serde_json::from_str::<BoundedVec<u8, 2>>("[1,2,3]").is_err());
}

fn capability_json() -> Value {
    json!({
        "schemaVersion": 1, "protocolVersion": "1", "newSession": true,
        "resumeSession": false, "loadSession": false, "closeSession": true,
        "prompt": true, "cancel": true, "nativeSessionFork": false,
        "activeTurnSteer": false, "childConversationProjection": false,
        "childTranscriptRead": false, "collaborationEventProjection": false,
        "multiRootExtensionSchemaId": null, "modelCount": 1, "configCount": 0
    })
}

#[test]
fn capability_and_activity_metadata_are_closed_and_bounded_at_ingress() {
    assert!(serde_json::from_value::<CapabilitySnapshotV1>(capability_json()).is_ok());
    assert!(
        serde_json::from_value::<CapabilitySnapshotV1>(with(
            capability_json(),
            json!({"modelCount": 129})
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<CapabilitySnapshotV1>(with(
            capability_json(),
            json!({"rawProvider": {}})
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ActivityProvenanceV1>(json!({
            "schemaVersion": 1, "sourceKind": "collaboration", "externalKind": "worker",
            "extensionSchemaId": null, "safeLabel": "research"
        }))
        .is_ok()
    );
    assert!(
        serde_json::from_value::<ActivityProvenanceV1>(json!({
            "schemaVersion": 2, "sourceKind": "tool", "externalKind": "worker",
            "extensionSchemaId": null, "safeLabel": null
        }))
        .is_err()
    );
}

#[test]
fn every_ipc_request_is_closed_and_uses_camel_case() {
    let query = json!({
        "projectId": "project-a", "projectGeneration": "3", "chatId": "chat-a", "cursor": null
    });
    assert!(serde_json::from_value::<GetAcpChatPageRequest>(query.clone()).is_ok());
    assert!(
        serde_json::from_value::<GetAcpChatPageRequest>(with(query, json!({"pageSize": 999})))
            .is_err()
    );

    let start = with(
        request_base(),
        json!({"bindingId": "fixture", "modelId": "model"}),
    );
    assert!(serde_json::from_value::<StartAcpSessionRequest>(start.clone()).is_ok());
    assert!(
        serde_json::from_value::<StartAcpSessionRequest>(with(
            start,
            json!({"executablePath": "/x"})
        ))
        .is_err()
    );

    let activate = with(
        request_base(),
        json!({"sessionId": "session-a", "runtimeEpoch": "2"}),
    );
    assert!(serde_json::from_value::<ActivateAcpSessionRequest>(activate.clone()).is_ok());
    assert!(
        serde_json::from_value::<ActivateAcpSessionRequest>(with(
            activate,
            json!({"arguments": []})
        ))
        .is_err()
    );

    let send = with(
        request_base(),
        json!({
            "target": "session", "sessionId": "session-a", "runtimeEpoch": "2",
            "prompt": "hello"
        }),
    );
    let parsed: SendAcpTurnRequest = serde_json::from_value(send.clone()).unwrap();
    assert!(matches!(
        parsed,
        SendAcpTurnRequest::Session { attached_context, .. }
            if attached_context.as_slice().is_empty()
    ));
    let context = json!({
        "kind": "diffLineComment",
        "diffRef": {
            "projectId": "project-a", "chatId": "chat-a", "sessionId": "session-a",
            "runtimeEpoch": "2", "turnId": "turn-a", "itemId": "item-a",
            "diffOrdinal": 0, "snapshotRevision": "1", "rootId": "root-a",
            "relativePath": "src/app.ts", "contentDigest": "a".repeat(64)
        },
        "side": "new", "startLine": "1", "endLine": "1", "note": "Review this"
    });
    let attached = with(send.clone(), json!({"attachedContext": [context.clone()]}));
    assert!(serde_json::from_value::<SendAcpTurnRequest>(attached).is_ok());
    assert!(serde_json::from_value::<SendAcpTurnRequest>(with(
        send.clone(),
        json!({"attachedContext": vec![context; crate::acp::limits::ATTACHED_CONTEXT_LIMIT + 1]})
    )).is_err());
    assert!(
        serde_json::from_value::<SendAcpTurnRequest>(with(send, json!({"queueLimit": 5}))).is_err()
    );
    let send_new = with(
        request_base(),
        json!({
            "target": "newSession", "bindingId": "fixture", "modelId": "model", "prompt": "hello"
        }),
    );
    assert!(matches!(
        serde_json::from_value::<SendAcpTurnRequest>(send_new).unwrap(),
        SendAcpTurnRequest::NewSession { .. }
    ));

    let cancel = with(
        request_base(),
        json!({
            "sessionId": "session-a", "runtimeEpoch": "2", "turnId": "turn-a",
            "expectedTurnState": "running", "expectedTurnRevision": "5"
        }),
    );
    assert!(serde_json::from_value::<CancelAcpTurnRequest>(cancel.clone()).is_ok());
    assert!(
        serde_json::from_value::<CancelAcpTurnRequest>(with(cancel, json!({"rawFrame": {}})))
            .is_err()
    );

    let close = with(
        request_base(),
        json!({"sessionId": "session-a", "runtimeEpoch": "2"}),
    );
    assert!(serde_json::from_value::<CloseAcpSessionRequest>(close.clone()).is_ok());
    assert!(
        serde_json::from_value::<CloseAcpSessionRequest>(with(close, json!({"environment": {}})))
            .is_err()
    );
}

#[test]
fn send_failure_serializes_prompt_dispatch_proof_without_changing_failure_kind() {
    let failure = SendAcpTurnFailure {
        failure: AcpIpcError::RecoveryRequired {
            message: SafeText::new("message", "snapshot failed").unwrap(),
        },
        prompt_dispatch: PromptDispatch::NotStarted,
    };
    assert_eq!(
        serde_json::to_value(failure).unwrap(),
        json!({
            "kind": "recoveryRequired",
            "message": "snapshot failed",
            "promptDispatch": "notStarted"
        })
    );
}

#[test]
fn exact_intent_validation_rejects_each_stale_or_foreign_component() {
    let current = snapshot();
    let mut cold = current.clone();
    cold.runtime_epoch = None;
    cold.turn_id = None;
    cold.turn_state = None;
    cold.turn_revision = None;
    let activate: ActivateAcpSessionRequest = serde_json::from_value(with(
        request_base(),
        json!({"sessionId": "session-a", "runtimeEpoch": "2"}),
    ))
    .unwrap();
    assert_eq!(validate_session_intent(&activate, &cold), Ok(()));

    let mut stale = cold.clone();
    stale.project_generation = ProjectGeneration::new(4).unwrap();
    assert_eq!(
        validate_session_intent(&activate, &stale),
        Err(IntentError::StaleProjectGeneration)
    );
    stale = cold.clone();
    stale.projection_revision = ProjectionRevision::new(8).unwrap();
    assert_eq!(
        validate_session_intent(&activate, &stale),
        Err(IntentError::StaleProjectionRevision)
    );
    stale = cold.clone();
    stale.runtime_epoch = Some(RuntimeEpoch::new(2).unwrap());
    assert_eq!(
        validate_session_intent(&activate, &stale),
        Err(IntentError::StaleRuntimeEpoch)
    );

    let cancel: CancelAcpTurnRequest = serde_json::from_value(with(
        request_base(),
        json!({
            "sessionId": "session-a", "runtimeEpoch": "2", "turnId": "turn-a",
            "expectedTurnState": "running", "expectedTurnRevision": "5"
        }),
    ))
    .unwrap();
    assert_eq!(validate_cancel_intent(&cancel, &current), Ok(()));
    stale = current.clone();
    stale.turn_state = Some(TurnState::Completed);
    assert_eq!(
        validate_cancel_intent(&cancel, &stale),
        Err(IntentError::UnexpectedTurnState)
    );
    stale = current.clone();
    stale.turn_revision = Some(EntityRevision::new(4).unwrap());
    assert_eq!(
        validate_cancel_intent(&cancel, &stale),
        Err(IntentError::StaleTurnRevision)
    );
    assert_eq!(
        validate_base_intent(
            &SageProjectId::parse("project-other").unwrap(),
            ProjectGeneration::new(3).unwrap(),
            &current.chat_id,
            current.projection_revision,
            &current
        ),
        Err(IntentError::ForeignProject)
    );
}

#[test]
fn cancel_intent_accepts_current_cancellable_turn_progress() {
    let current = snapshot();
    let cancel: CancelAcpTurnRequest = serde_json::from_value(with(
        request_base(),
        json!({
            "sessionId": "session-a", "runtimeEpoch": "2", "turnId": "turn-a",
            "expectedTurnState": "sending", "expectedTurnRevision": "5"
        }),
    ))
    .unwrap();

    let mut advanced = current.clone();
    advanced.projection_revision = ProjectionRevision::new(8).unwrap();
    advanced.turn_state = Some(TurnState::Running);
    advanced.turn_revision = Some(EntityRevision::new(6).unwrap());
    assert_eq!(validate_cancel_intent(&cancel, &advanced), Ok(()));

    let mut future_projection = cancel.clone();
    future_projection.expected_projection_revision = ProjectionRevision::new(9).unwrap();
    assert_eq!(
        validate_cancel_intent(&future_projection, &advanced),
        Err(IntentError::StaleProjectionRevision)
    );

    let mut drifted = advanced.clone();
    drifted.turn_state = Some(TurnState::Queued);
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::UnexpectedTurnState)
    );
    drifted = advanced.clone();
    drifted.project_id = SageProjectId::parse("project-other").unwrap();
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::ForeignProject)
    );
    drifted = advanced.clone();
    drifted.project_generation = ProjectGeneration::new(4).unwrap();
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::StaleProjectGeneration)
    );
    drifted = advanced.clone();
    drifted.chat_id = SageChatId::parse("chat-other").unwrap();
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::ForeignChat)
    );
    drifted = advanced.clone();
    drifted.session_id = Some(SageAgentSessionId::parse("session-other").unwrap());
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::ForeignSession)
    );
    drifted = advanced.clone();
    drifted.runtime_epoch = Some(RuntimeEpoch::new(3).unwrap());
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::StaleRuntimeEpoch)
    );
    drifted = advanced.clone();
    drifted.turn_id = Some(SageTurnId::parse("turn-other").unwrap());
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::ForeignTurn)
    );
    drifted = advanced.clone();
    drifted.turn_state = Some(TurnState::Completed);
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::UnexpectedTurnState)
    );
    drifted = advanced.clone();
    drifted.turn_revision = Some(EntityRevision::new(4).unwrap());
    assert_eq!(
        validate_cancel_intent(&cancel, &drifted),
        Err(IntentError::StaleTurnRevision)
    );
}

#[test]
fn steer_intent_requires_the_exact_live_turn_and_bounded_prompt() {
    let request = with(
        request_base(),
        json!({
            "sessionId": "session-a", "runtimeEpoch": "2", "turnId": "turn-a",
            "expectedTurnRevision": "5", "prompt": "Change direction"
        }),
    );
    let steer: SteerAcpTurnRequest = serde_json::from_value(request.clone()).unwrap();
    let current = snapshot();
    assert_eq!(validate_steer_intent(&steer, &current), Ok(()));
    let mut stale = current.clone();
    stale.turn_id = Some(SageTurnId::parse("turn-other").unwrap());
    assert_eq!(
        validate_steer_intent(&steer, &stale),
        Err(IntentError::ForeignTurn)
    );
    stale = current;
    stale.turn_state = Some(TurnState::Completed);
    assert_eq!(
        validate_steer_intent(&steer, &stale),
        Err(IntentError::UnexpectedTurnState)
    );
    assert!(
        serde_json::from_value::<SteerAcpTurnRequest>(with(
            request.clone(),
            json!({"prompt": "x".repeat(100_001)})
        ))
        .is_err()
    );
    assert!(
        serde_json::from_value::<SteerAcpTurnRequest>(with(request, json!({"rawFrame": {}})))
            .is_err()
    );
}

#[test]
fn projection_events_are_revision_only_and_positive() {
    let event: ProjectionChangedV1 = serde_json::from_value(json!({
        "projectId": "project-a", "chatId": "chat-a", "sessionId": "session-a",
        "projectionRevision": "1"
    }))
    .unwrap();
    assert_eq!(event.validate(), Ok(()));
    let zero: ProjectionChangedV1 = serde_json::from_value(json!({
        "projectId": "project-a", "chatId": "chat-a", "sessionId": "session-a",
        "projectionRevision": "0"
    }))
    .unwrap();
    assert_eq!(zero.validate(), Err(IntentError::InvalidProjectionRevision));
    assert!(
        serde_json::from_value::<ProjectionChangedV1>(json!({
            "projectId": "project-a", "chatId": "chat-a", "sessionId": "session-a",
            "projectionRevision": "1", "items": []
        }))
        .is_err()
    );
}

#[test]
fn native_chat_page_serializes_the_typescript_wire_shape() {
    let project_id = SageProjectId::parse("project-a").unwrap();
    let chat_id = SageChatId::parse("chat-a").unwrap();
    let session_id = SageAgentSessionId::parse("session-a").unwrap();
    let turn_id = SageTurnId::parse("turn-a").unwrap();
    let item = ItemProjection {
        id: SageItemId::parse("item-a").unwrap(),
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session_id.clone(),
        turn_id: turn_id.clone(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        ordinal: 0,
        origin: ItemOrigin::Primary,
        activity_id: None,
        lifecycle: ItemLifecycle::Completed,
        revision: EntityRevision::new(3).unwrap(),
        created_at: 10,
        updated_at: 11,
        terminal_at: Some(11),
        tool_state: None,
        content: ItemContent::Text {
            role: TextRole::Assistant,
            text: SafeText::new("text", "hello").unwrap(),
        },
    };
    let turn = TurnProjection {
        id: turn_id,
        session_id: session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        ordinal: 0,
        state: TurnState::Completed,
        revision: EntityRevision::new(3).unwrap(),
        created_at: 10,
        updated_at: 11,
        terminal_at: Some(11),
        usage: None,
        terminal_reason: None,
        items: BoundedVec::new("items", vec![item]).unwrap(),
        activities: BoundedVec::new("activities", vec![]).unwrap(),
        browser_operations: BoundedVec::new("browserOperations", vec![]).unwrap(),
        browser_operations_collapsed: 0,
    };
    let session = SessionProjection {
        id: session_id.clone(),
        ordinal: 0,
        binding_id: RequiredText::new("bindingId", "fixture").unwrap(),
        model_id: None,
        state: SessionState::Closed,
        residency: Residency::Cold,
        runtime_epoch: None,
        next_runtime_epoch: None,
        permission_requests: false,
        capabilities: CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocolVersion", "fixture-v1").unwrap(),
            new_session: true,
            resume_session: false,
            load_session: false,
            close_session: true,
            prompt: true,
            cancel: true,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: false,
            child_transcript_read: false,
            collaboration_event_projection: false,
            multi_root_extension_schema_id: None,
            model_count: 1,
            config_count: 0,
        },
        revision: EntityRevision::new(3).unwrap(),
        projection_revision: ProjectionRevision::new(4).unwrap(),
        legacy: false,
    };
    let page = AcpChatPage {
        session_config: None,
        project_id,
        chat_id,
        title: SafeText::new("title", "Wire fixture").unwrap(),
        projection_revision: ProjectionRevision::new(4).unwrap(),
        current_session_id: Some(session_id),
        sessions: BoundedVec::new("sessions", vec![session]).unwrap(),
        turns: BoundedVec::new("turns", vec![turn]).unwrap(),
        next_cursor: None,
    };
    assert_eq!(page.validate_page_bounds(), Ok(()));

    let value = serde_json::to_value(page).unwrap();
    assert_eq!(value["projectionRevision"], "4");
    assert_eq!(value["turns"][0]["runtimeEpoch"], "2");
    assert_eq!(value["turns"][0]["items"][0]["kind"], "text");
    assert_eq!(
        value["turns"][0]["items"][0]["payload"],
        json!({
            "role": "assistant", "text": "hello"
        })
    );
    assert!(value.get("items").is_none());
    assert!(value.get("session").is_none());
}
