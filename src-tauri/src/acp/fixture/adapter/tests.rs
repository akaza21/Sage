use crate::acp::{
    bindings::adapter::{
        AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
    },
    transcript_quote::Prompt,
    transport::IncrementalFrameDecoder,
};
use sage_acp_protocol::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SchemaVersionV1},
    intent::{ExternalId, SafeResourceRef, UnavailableResourceReason},
};

use super::{AdapterRuntimeError, FixtureAdapter, ingest_metadata};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

#[test]
fn metadata_notification_rejects_explicit_null_id() {
    let result = ingest_metadata(serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "method": "fixture/metadata",
        "params": {"_meta": {}}
    }));

    assert!(matches!(result, Err(AdapterRuntimeError::Frame)));
}

#[test]
fn activation_capabilities_match_the_implemented_session_surface() {
    let capabilities = FixtureAdapter::activation_capabilities();
    assert_eq!(capabilities.schema_version, SchemaVersionV1);
    assert_eq!(capabilities.protocol_version.as_str(), "fixture-1");
    assert_eq!(
        (
            capabilities.new_session,
            capabilities.resume_session,
            capabilities.load_session,
            capabilities.close_session,
            capabilities.prompt,
            capabilities.cancel,
        ),
        (true, true, false, false, true, true)
    );
    assert_eq!(
        (
            capabilities.native_session_fork,
            capabilities.active_turn_steer,
            capabilities.child_conversation_projection,
            capabilities.child_transcript_read,
            capabilities.collaboration_event_projection,
        ),
        (false, false, false, false, false)
    );
    assert_eq!(
        (capabilities.model_count, capabilities.config_count),
        (0, 0)
    );
    assert!(capabilities.multi_root_extension_schema_id.is_none());
}

#[test]
fn initialize_then_resume_preserves_the_persisted_session_identity() {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut adapter = adapter(external.clone());

    write_command(&mut adapter, AdapterCommand::Initialize);
    let initialized = adapter
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    assert!(matches!(
        initialized.events(),
        [AdapterEvent::Initialized { capabilities }] if capabilities.resume_session
    ));

    let resume = adapter
        .command(AdapterCommand::ResumeSession {
            external_session_id: external.clone(),
            native_root: "C:\\fixture".into(),
            model: None,
        })
        .unwrap();
    let wire: serde_json::Value = serde_json::from_slice(resume.frame().payload()).unwrap();
    assert_eq!(wire["method"], "fixture/resume");
    assert_eq!(wire["params"]["externalSessionId"], external.as_str());
    adapter
        .resolve_egress(resume.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(resume.into_token(), EgressResolution::Written)
        .unwrap();

    let ready = adapter
        .ingest(
            &frame(
                "resumeSuccess",
                serde_json::json!({"externalSessionId":external.as_str()}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        ready.events(),
        [AdapterEvent::SessionReady { id, loaded: false }]
            if id.external_id == external && id.turn_id.is_none()
                && id.turn_client_message_id.is_none()
    ));
}

#[test]
fn initialize_then_new_session_uses_the_store_bound_identity() {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut adapter = adapter(external.clone());

    write_command(&mut adapter, AdapterCommand::Initialize);
    adapter
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::NewSession {
            native_root: "C:\\fixture".into(),
            model: None,
        },
    );

    let ready = adapter
        .ingest(
            &frame(
                "newSessionSuccess",
                serde_json::json!({"externalSessionId": external.as_str()}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        ready.events(),
        [AdapterEvent::SessionReady { id, loaded: false }]
            if id.external_id == external && id.turn_id.is_none()
                && id.turn_client_message_id.is_none()
    ));
}

#[test]
fn cancel_after_activation_uses_the_next_request_identity() {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut adapter = adapter(external.clone());

    write_command(&mut adapter, AdapterCommand::Initialize);
    adapter
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::NewSession {
            native_root: "C:\\fixture".into(),
            model: None,
        },
    );
    adapter
        .ingest(
            &frame(
                "newSessionSuccess",
                serde_json::json!({"externalSessionId": external.as_str()}),
            ),
            &Mapper,
        )
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "fixture cancel setup").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        },
    );

    let cancel = adapter.command(AdapterCommand::Cancel).unwrap();
    assert!(cancel.token().is_cancel());
    assert_eq!(cancel.token().request_id(), Some(4));
    adapter
        .resolve_egress(cancel.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();
    let terminal = adapter
        .ingest(&frame("turnCancelled", serde_json::json!({})), &Mapper)
        .unwrap();
    assert!(matches!(
        terminal.events(),
        [AdapterEvent::Turn {
            state: sage_acp_protocol::lifecycle::TurnState::Cancelled,
            ..
        }]
    ));
}

#[test]
fn output_before_cancel_converges_to_cancelled() {
    let mut adapter = streaming_adapter();

    let cancel = adapter.command(AdapterCommand::Cancel).unwrap();
    adapter
        .resolve_egress(cancel.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();
    let terminal = adapter
        .ingest(&frame("turnCancelled", serde_json::json!({})), &Mapper)
        .unwrap();
    assert!(matches!(
        terminal.events(),
        [AdapterEvent::Turn {
            state: sage_acp_protocol::lifecycle::TurnState::Cancelled,
            ..
        }]
    ));
}

#[test]
fn failed_streaming_cancel_delivery_restores_the_turn() {
    for resolutions in [
        &[EgressResolution::QueueRejected][..],
        &[EgressResolution::Queued, EgressResolution::WriteZeroBytes][..],
    ] {
        let mut adapter = streaming_adapter();
        let cancel = adapter.command(AdapterCommand::Cancel).unwrap();
        let token = cancel.into_token();
        for resolution in resolutions {
            adapter.resolve_egress(token.clone(), *resolution).unwrap();
        }

        let delta = adapter
            .ingest(&frame("textDelta", serde_json::json!({})), &Mapper)
            .unwrap();
        assert!(matches!(
            delta.events(),
            [AdapterEvent::Item {
                lifecycle: sage_acp_protocol::lifecycle::ItemLifecycle::Streaming,
                ..
            }]
        ));
        assert!(adapter.command(AdapterCommand::Cancel).is_ok());
    }
}

fn streaming_adapter() -> FixtureAdapter {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut adapter = adapter(external.clone());
    write_command(&mut adapter, AdapterCommand::Initialize);
    adapter
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::NewSession {
            native_root: "C:\\fixture".into(),
            model: None,
        },
    );
    adapter
        .ingest(
            &frame(
                "newSessionSuccess",
                serde_json::json!({"externalSessionId": external.as_str()}),
            ),
            &Mapper,
        )
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "fixture cold cycle").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        },
    );

    let item = adapter
        .ingest(&frame("textStart", serde_json::json!({})), &Mapper)
        .unwrap();
    assert!(matches!(
        item.events(),
        [
            AdapterEvent::Turn {
                state: sage_acp_protocol::lifecycle::TurnState::Running,
                ..
            },
            AdapterEvent::Item {
                lifecycle: sage_acp_protocol::lifecycle::ItemLifecycle::Started,
                ..
            }
        ]
    ));
    adapter
}

#[test]
fn activation_responses_reject_an_explicit_null_id() {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut initialize = adapter(external.clone());
    write_command(&mut initialize, AdapterCommand::Initialize);
    assert!(matches!(
        initialize.ingest(
            &frame_with_id(
                "initializeSuccess",
                serde_json::json!({}),
                serde_json::Value::Null
            ),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Frame)
    ));

    let mut resume = adapter(external.clone());
    write_command(&mut resume, AdapterCommand::Initialize);
    resume
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    write_command(
        &mut resume,
        AdapterCommand::ResumeSession {
            external_session_id: external.clone(),
            native_root: "C:\\fixture".into(),
            model: None,
        },
    );
    assert!(matches!(
        resume.ingest(
            &frame_with_id(
                "resumeSuccess",
                serde_json::json!({"externalSessionId": external.as_str()}),
                serde_json::Value::Null,
            ),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Frame)
    ));
}

#[test]
fn resume_failure_cannot_issue_a_second_resume_or_prompt() {
    let external = ExternalId::new("externalSession", "fixture-persisted-session-1").unwrap();
    let mut adapter = adapter(external.clone());
    write_command(&mut adapter, AdapterCommand::Initialize);
    adapter
        .ingest(&frame("initializeSuccess", serde_json::json!({})), &Mapper)
        .unwrap();
    write_command(
        &mut adapter,
        AdapterCommand::ResumeSession {
            external_session_id: external.clone(),
            native_root: "C:\\fixture".into(),
            model: None,
        },
    );

    let failed = adapter
        .ingest(&frame("resumeFailure", serde_json::json!({})), &Mapper)
        .unwrap();
    assert!(matches!(failed.events(), [AdapterEvent::Error(_)]));
    assert!(matches!(
        adapter.command(AdapterCommand::ResumeSession {
            external_session_id: external,
            native_root: "C:\\fixture".into(),
            model: None,
        }),
        Err(AdapterRuntimeError::Operation)
    ));
    assert!(matches!(
        adapter.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "must not replay").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::Operation)
    ));
}

fn adapter(external: ExternalId) -> FixtureAdapter {
    FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(2).unwrap(),
        SageAgentSessionId::parse("fixture-session").unwrap(),
        external,
    )
    .unwrap()
}

fn write_command(adapter: &mut FixtureAdapter, command: AdapterCommand) {
    let egress = adapter.command(command).unwrap();
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
}

fn frame(method: &str, params: serde_json::Value) -> crate::acp::transport::Frame {
    frame_value(serde_json::json!({
        "jsonrpc": "2.0",
        "method": format!("fixture/{method}"),
        "params": params,
    }))
}

fn frame_with_id(
    method: &str,
    params: serde_json::Value,
    id: serde_json::Value,
) -> crate::acp::transport::Frame {
    frame_value(serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": format!("fixture/{method}"),
        "params": params,
    }))
}

fn frame_value(value: serde_json::Value) -> crate::acp::transport::Frame {
    let bytes = serde_json::to_vec(&value).unwrap();
    IncrementalFrameDecoder::new()
        .push_chunk(&[bytes, b"\n".to_vec()].concat())
        .unwrap()
        .unwrap()
}
