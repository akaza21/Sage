use crate::acp::{
    bindings::adapter::{
        AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    intent::{ExternalId, Prompt, SafeResourceRef, UnavailableResourceReason},
    transport::IncrementalFrameDecoder,
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
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": format!("fixture/{method}"),
        "params": params,
    }))
    .unwrap();
    IncrementalFrameDecoder::new()
        .push_chunk(&[bytes, b"\n".to_vec()].concat())
        .unwrap()
        .unwrap()
}
