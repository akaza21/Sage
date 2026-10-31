use super::{extended_tests::*, *};
use crate::acp::{
    contract::{ClientIntentId, RequiredText, SchemaVersionV1},
    intent::{ActivityProvenanceV1, ActivitySourceKind, ExternalId, NormalizedPayload},
    lifecycle::{ActivityLifecycle, ItemLifecycle},
    projection::ActivityRole,
};

#[test]
fn normalized_item_and_activity_ingress_are_exact_idempotent_and_terminal_safe() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-ingress")).unwrap();
    activate_session(
        &mut connection,
        ActivateSessionCommand {
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            expected_projection_revision: ProjectionRevision::new(1).unwrap(),
            client_intent_id: ClientIntentId::parse("activate-ingress").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "d".repeat(64),
            now_millis: 2,
        },
    )
    .unwrap();
    complete_activation(
        &mut connection,
        CompleteActivationCommand {
            model_context: Default::default(),
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: ExternalId::new("external", "ingress-session").unwrap(),
            capabilities_json: capabilities(),
            now_millis: 3,
        },
    )
    .unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-ingress','project-one','chat-one',?1,1,0,'turn-ingress-intent','running','correlated',0,4,4,NULL)",[started.session_id.as_str()]).unwrap();
    let payload = NormalizedPayload::Text {
        text: crate::acp::contract::SafeText::new("text", "hello").unwrap(),
    };
    let payload_json = serde_json::to_string(&payload).unwrap();
    let command = ItemIngressCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: started.session_id.clone(),
        turn_id: SageTurnId::parse("turn-ingress").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        external_item_id: ExternalId::new("external", "item-external").unwrap(),
        activity_id: None,
        payload,
        payload_digest: super::facts::hex_digest(payload_json.as_bytes()),
        lifecycle: ItemLifecycle::Started,
        tool_state: None,
        now_millis: 5,
    };
    let item = apply_item_ingress(&mut connection, command.clone()).unwrap();
    assert_eq!(
        apply_item_ingress(&mut connection, command.clone())
            .unwrap()
            .entity
            .disposition,
        MutationDisposition::Duplicate
    );
    let provenance = ActivityProvenanceV1 {
        schema_version: SchemaVersionV1,
        source_kind: ActivitySourceKind::Collaboration,
        external_kind: RequiredText::new("kind", "subagent").unwrap(),
        extension_schema_id: None,
        safe_label: Some(RequiredText::new("label", "Worker").unwrap()),
    };
    let provenance_json = serde_json::to_string(&provenance).unwrap();
    let activity = ActivityIngressCommand {
        project_id: command.project_id.clone(),
        chat_id: command.chat_id.clone(),
        session_id: command.session_id.clone(),
        turn_id: command.turn_id.clone(),
        runtime_epoch: command.runtime_epoch,
        external_activity_id: ExternalId::new("external", "activity-external").unwrap(),
        activity_kind: RequiredText::new("kind", "subagent").unwrap(),
        parent_activity_id: None,
        parent_item_id: item.item_id,
        role: ActivityRole::Child,
        lifecycle: ActivityLifecycle::Observed,
        depth: 1,
        observed_sequence: 1,
        provenance,
        provenance_digest: super::facts::hex_digest(provenance_json.as_bytes()),
        now_millis: 6,
    };
    assert_eq!(
        apply_activity_ingress(&mut connection, activity.clone())
            .unwrap()
            .entity
            .disposition,
        MutationDisposition::Applied
    );
    assert_eq!(
        apply_activity_ingress(&mut connection, activity)
            .unwrap()
            .entity
            .disposition,
        MutationDisposition::Duplicate
    );
    let terminal = ItemIngressCommand {
        lifecycle: ItemLifecycle::Completed,
        now_millis: 7,
        ..command
    };
    assert_eq!(
        apply_item_ingress(&mut connection, terminal.clone())
            .unwrap()
            .entity
            .disposition,
        MutationDisposition::Applied
    );
    assert!(matches!(
        apply_item_ingress(
            &mut connection,
            ItemIngressCommand {
                lifecycle: ItemLifecycle::Failed,
                ..terminal
            }
        ),
        Err(StoreError::TerminalConflict { resource: "item" })
    ));
}
