use super::*;
use crate::acp::bindings::pi::AdapterOutput;
use std::sync::{Arc, Mutex};

struct SaturatedRefresh {
    runtime: PiRuntime,
    fill_prompt_slot: bool,
    filled: Arc<Mutex<Option<Value>>>,
}

impl HarnessAdapter for SaturatedRefresh {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.runtime.capabilities()
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.runtime.session_config()
    }
    fn config_pending(&self) -> bool {
        self.runtime.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.runtime.configure(intent)
    }
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.runtime.command(command)
    }
    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if !std::mem::take(&mut self.fill_prompt_slot) {
            return self.runtime.ingest(frame, resources);
        }
        let outputs = self.runtime.translator.ingest(frame.as_bytes()).unwrap();
        assert!(matches!(
            outputs.as_slice(),
            [AdapterOutput::CommandAccepted { command: "prompt" }]
        ));
        let command = self
            .runtime
            .begin_one(Operation::GetModels, RequestKind::Models)
            .unwrap();
        *self.filled.lock().unwrap() = Some(deliver(&mut self.runtime, command));
        assert_eq!(self.runtime.translator.pending_count(), 16);
        let digest = crate::acp::transport::request_payload_digest(frame).unwrap();
        self.runtime
            .translate_outputs(outputs, None, digest, resources)
    }
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.runtime.commit_mandatory_egress(token)
    }
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.runtime.resolve_egress(token, resolution)
    }
}

#[tokio::test]
async fn pi_pump_optional_refresh_skips_full_command_capacity_and_retries() {
    let fixture = Fixture::new();
    let mut runtime = ready_adapter().inner;
    runtime.refresh_thinking_after_model = true;
    let mut pending = Vec::new();
    for _ in 0..15 {
        let command = runtime
            .begin_one(Operation::GetModels, RequestKind::Models)
            .unwrap();
        pending.push(deliver(&mut runtime, command));
    }
    let filled = Arc::new(Mutex::new(None));
    let mut pump = fixture.pump_adapter(SaturatedRefresh {
        runtime,
        fill_prompt_slot: true,
        filled: filled.clone(),
    });
    start(&mut pump).await;
    let mut skipped = Capture::default();
    pump.flush_one(&mut skipped).unwrap();
    assert!(
        skipped.0.is_empty(),
        "optional refresh must skip saturated capacity"
    );
    pending.push(filled.lock().unwrap().take().unwrap());
    for command in pending {
        pump.push_stdout_chunk(
            &reply(
                &command,
                json!({"models":[
                    {"provider":"openai","id":"gpt","name":"GPT"}
                ]}),
            ),
            5,
        )
        .await
        .unwrap();
    }
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0.len(), 1);
    assert_eq!(
        transport.0[0]["type"], "get_state",
        "deferred refresh survives capacity failure"
    );
    pump.push_stdout_chunk(
        &reply(
            &transport.0[0],
            json!({
                "model":{"provider":"openai","id":"gpt"},"thinkingLevel":"high"
            }),
        ),
        6,
    )
    .await
    .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0.len(), 2);
    assert_eq!(transport.0[1]["type"], "get_available_thinking_levels");
    pump.push_stdout_chunk(&reply(&transport.0[1], json!({"levels":["high","max"]})), 7)
        .await
        .unwrap();
    assert!(!pump.config_pending());
    settle(&mut pump).await;
    assert!(!pump.teardown_required());
    fixture.finish().await;
}
