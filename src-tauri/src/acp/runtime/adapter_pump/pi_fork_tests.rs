//! Pi 1.5.0's captured clone sequence through the completed-turn pump.
use super::*;
use crate::acp::{
    bindings::{pi::PiRuntime, runtime::NativeRuntime},
    goal::NativeRpcReply,
};
use serde_json::{Value, json};

const HOME: &str = "2026-11-13T03-13-45-610Z_01a1d2e5-9c8a-74ad-872c-8747e8d4df99";
const COPY: &str = "2026-11-13T03-15-47-158Z_01a1d2e7-7756-74ad-872c-8747e8d4df99";

struct Sessions(std::path::PathBuf);

impl Sessions {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("sage-pi-fork-pump-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn file(&self, key: &str) -> std::path::PathBuf {
        self.0.join(format!("{key}.jsonl"))
    }

    fn state(&self, key: &str) -> Value {
        json!({"sessionFile":self.file(key), "sessionId":key.split_once('_').unwrap().1,
            "isStreaming":false, "thinkingLevel":"high",
            "model":{"provider":"openai","id":"gpt","name":"GPT"}})
    }
}

impl Drop for Sessions {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn response_line(request: &Value, data: Value) -> Vec<u8> {
    codex_line(json!({"type":"response", "id":request["id"],
        "command":request["type"], "success":true, "data":data}))
}

fn written(adapter: &mut NativeRuntime, egress: AdapterEgress) -> Value {
    let request = serde_json::from_slice(egress.frame().payload()).unwrap();
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    request
}

fn answer(adapter: &mut NativeRuntime, request: &Value, data: Value) -> Option<Value> {
    let bytes = response_line(request, data);
    let frame = IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap();
    let (_, mut egress) = adapter.ingest(&frame, &Mapper).unwrap().into_parts();
    assert!(egress.len() <= 1);
    egress.pop().map(|egress| written(adapter, egress))
}

fn ready_pi(files: &Sessions) -> NativeRuntime {
    let mut adapter = NativeRuntime::new(PiRuntime::fixture(
        RuntimeEpoch::new(1).unwrap(),
        session("one"),
    ));
    let init = adapter.command(AdapterCommand::Initialize).unwrap();
    let init = written(&mut adapter, init);
    let models = answer(&mut adapter, &init, files.state(HOME)).unwrap();
    assert!(
        answer(
            &mut adapter,
            &models,
            json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]})
        )
        .is_none()
    );
    let open = adapter
        .command(AdapterCommand::NewSession {
            native_root: files.0.to_str().unwrap().into(),
            model: None,
        })
        .unwrap();
    let open = written(&mut adapter, open);
    assert!(answer(&mut adapter, &open, files.state(HOME)).is_none());
    std::fs::write(files.file(HOME), "{}\n").unwrap();
    adapter
}

async fn completed_pi(files: &Sessions) -> CompletedPumpParts<NativeRuntime, FakeJournal, Mapper> {
    let mut adapter = ready_pi(files);
    let prompt = adapter
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: message(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt = written(&mut adapter, prompt);
    let state = answer(&mut adapter, &prompt, Value::Null).unwrap();
    let levels = answer(&mut adapter, &state, files.state(HOME)).unwrap();
    assert!(answer(&mut adapter, &levels, json!({"levels":["off","high"]})).is_none());
    let shared = Shared(Arc::new(Mutex::new(FakeState::default())));
    let mut scope = session_intent();
    scope.external_session_id = ExternalId::new("session", HOME).unwrap();
    let mut pump = AdapterPump::new(
        adapter,
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
        Arc::new(FakePublisher {
            shared,
            outcomes: Mutex::new(VecDeque::new()),
        }),
        scope.bind_turn(ActiveTurnIntent {
            turn_id: turn("one"),
            external_turn_id: None,
            client_message_id: message(),
        }),
    )
    .unwrap();
    for event in [
        json!({"type":"agent_start"}),
        json!({"type":"agent_end","messages":[]}),
        json!({"type":"agent_settled"}),
    ] {
        pump.push_stdout_chunk(&codex_line(event), 1).await.unwrap();
    }
    pump.try_into_completed_parts()
        .ok()
        .expect("finished Pi turn")
}

fn take_request(output: &mut CaptureTransport, command: &str) -> Value {
    let value: Value = serde_json::from_slice(&std::mem::take(&mut output.0)).unwrap();
    assert_eq!(value["type"], command);
    value
}

#[test]
fn pi_150_clone_replies_fork_a_finished_turn_through_the_runtime_pump() {
    runtime().block_on(async {
        let files = Sessions::new();
        let mut idle = completed_pi(&files).await;
        let mut output = CaptureTransport(Vec::new());
        idle.write_fork(&mut output).unwrap();
        let clone = take_request(&mut output, "clone");
        std::fs::write(files.file(COPY), "{}\n").unwrap();
        idle.accept_idle_stdout_chunk(
            &response_line(&clone, json!({"cancelled":false})),
            2,
            &mut output,
        )
        .await
        .expect("clone reply must write get_state, not require recovery");
        let state = take_request(&mut output, "get_state");
        assert!(idle.native_rpc_pending());
        assert_eq!(idle.take_native_rpc_reply(), None);
        idle.accept_idle_stdout_chunk(&response_line(&state, files.state(COPY)), 3, &mut output)
            .await
            .unwrap();
        let back = take_request(&mut output, "switch_session");
        assert_eq!(back["sessionPath"], files.file(HOME).to_str().unwrap());
        // A duplicate earlier reply cannot consume the return's awaited id.
        idle.accept_idle_stdout_chunk(
            &response_line(&clone, json!({"cancelled":false})),
            4,
            &mut output,
        )
        .await
        .unwrap();
        assert!(idle.native_rpc_pending());
        idle.accept_idle_stdout_chunk(
            &response_line(&back, json!({"cancelled":false})),
            5,
            &mut output,
        )
        .await
        .unwrap();
        assert!(!idle.native_rpc_pending());
        let Some(NativeRpcReply::Forked(fork)) = idle.take_native_rpc_reply() else {
            panic!("confirmed fork")
        };
        assert_eq!(fork.as_str(), COPY);
        idle.write_release_fork(&mut output, &fork, false).unwrap();
        assert_eq!(idle.take_native_rpc_reply(), Some(NativeRpcReply::Released));
        assert!(!files.file(COPY).exists());
        assert!(files.file(HOME).is_file());
    });
}

#[test]
fn pi_fork_return_failure_does_not_lose_the_known_copy() {
    runtime().block_on(async {
        let files = Sessions::new();
        let mut idle = completed_pi(&files).await;
        let mut output = CaptureTransport(Vec::new());
        idle.write_fork(&mut output).unwrap();
        let clone = take_request(&mut output, "clone");
        std::fs::write(files.file(COPY), "{}\n").unwrap();
        idle.accept_idle_stdout_chunk(
            &response_line(&clone, json!({"cancelled":false})),
            2,
            &mut output,
        )
        .await
        .unwrap();
        let state = take_request(&mut output, "get_state");
        idle.accept_idle_stdout_chunk(&response_line(&state, files.state(COPY)), 3, &mut output)
            .await
            .unwrap();
        let back = take_request(&mut output, "switch_session");
        assert!(
            idle.accept_idle_stdout_chunk(
                &response_line(&back, json!({"cancelled":true})),
                4,
                &mut output
            )
            .await
            .is_err()
        );
        assert_eq!(idle.take_native_rpc_reply(), None);
        assert!(
            !files.file(COPY).exists(),
            "recovery must clean the refused fork"
        );
        assert!(files.file(HOME).is_file());
    });
}

#[test]
fn pi_150_clone_replies_also_fork_through_the_activation_idle_pump() {
    use crate::acp::runtime::{ActivatedSession, IdleActivationParts};
    let files = Sessions::new();
    let adapter = ready_pi(&files);
    let scope = session_intent();
    let activated = ActivatedSession {
        binding_digest: scope.binding_digest,
        project_id: scope.project_id,
        chat_id: scope.chat_id,
        session_id: scope.session_id,
        runtime_epoch: scope.runtime_epoch,
        external_session_id: ExternalId::new("session", HOME).unwrap(),
        capabilities: adapter.capabilities().unwrap(),
        models: Vec::new(),
        next_model_cursor: None,
    };
    let mut idle = IdleActivationParts::new(adapter, (), Mapper, activated);
    let mut output = CaptureTransport(Vec::new());
    idle.write_fork(&mut output).unwrap();
    std::fs::write(files.file(COPY), "{}\n").unwrap();
    for (command, data) in [
        ("clone", json!({"cancelled":false})),
        ("get_state", files.state(COPY)),
        ("switch_session", json!({"cancelled":false})),
    ] {
        let request = take_request(&mut output, command);
        if command == "switch_session" {
            assert_eq!(request["sessionPath"], files.file(HOME).to_str().unwrap());
        }
        let frame = IncrementalFrameDecoder::new()
            .push_chunk(&response_line(&request, data))
            .unwrap()
            .unwrap();
        let batch = idle.ingest_idle_frame(&frame).unwrap();
        idle.accept_idle_batch(batch, &mut output).unwrap();
    }
    let Some(NativeRpcReply::Forked(fork)) = idle.take_native_rpc_reply() else {
        panic!("confirmed fork")
    };
    assert_eq!(fork.as_str(), COPY);
    idle.write_release_fork(&mut output, &fork, false).unwrap();
    assert_eq!(idle.take_native_rpc_reply(), Some(NativeRpcReply::Released));
    assert!(!files.file(COPY).exists());
    assert!(files.file(HOME).is_file());
}

#[test]
fn runtime_disposal_cleans_only_unconfirmed_pi_forks() {
    runtime().block_on(async {
        for confirmed in [false, true] {
            let files = Sessions::new();
            let mut idle = completed_pi(&files).await;
            let mut output = CaptureTransport(Vec::new());
            idle.write_fork(&mut output).unwrap();
            let clone = take_request(&mut output, "clone");
            std::fs::write(files.file(COPY), "{}\n").unwrap();
            idle.accept_idle_stdout_chunk(
                &response_line(&clone, json!({"cancelled":false})),
                2,
                &mut output,
            )
            .await
            .unwrap();
            let state = take_request(&mut output, "get_state");
            idle.accept_idle_stdout_chunk(
                &response_line(&state, files.state(COPY)),
                3,
                &mut output,
            )
            .await
            .unwrap();
            let back = take_request(&mut output, "switch_session");
            if confirmed {
                idle.accept_idle_stdout_chunk(
                    &response_line(&back, json!({"cancelled":false})),
                    4,
                    &mut output,
                )
                .await
                .unwrap();
            }
            // EOF/teardown before the return reply produces RecoveryRequired.
            // Once confirmed, however, the caller may be committing its Chat.
            drop(idle);
            assert_eq!(files.file(COPY).exists(), confirmed);
            assert!(files.file(HOME).is_file());
        }
    });
}
