use super::*;

fn poll(runtime: &mut NativeRuntime) -> Value {
    let egress = runtime.poll_title().expect("due read");
    serde_json::from_slice(egress.frame().payload()).unwrap()
}

#[test]
fn existing_titles_keep_short_phrases_issue_identifiers_and_chinese_text() {
    for title in ["Fix tests", "Fix issue #443", "修复测试失败"] {
        let mut fixture = running("existing-title");
        fixture.runtime.start_title("").unwrap();
        let request = poll(&mut fixture.runtime);
        fixture.runtime.ingest(&frame(json!({"id":request["id"],"result":{"sessions":[{"sessionId":SESSION,"title":title}]}})), &Mapper).unwrap();
        assert_eq!(
            fixture.runtime.take_title_result(),
            Some(Some(title.into()))
        );
    }
}

#[test]
fn oversized_title_pages_through_the_idle_pump_leave_foreground_prompts_usable() {
    use crate::acp::runtime::{
        ActivatedSession, CompletedActivationDriverParts, EgressTransport, IdleActivationParts,
        StdinTransport, SystemDriverClock,
    };
    use crate::acp::transport::{FrameWriteOutcome, OutboundFrame};
    use sage_acp_protocol::{
        contract::{SageAgentSessionId, SageChatId},
        intent::ExternalId,
        scheduler::RuntimeOwner,
    };
    struct Transport;
    impl EgressTransport for Transport {
        fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
            FrameWriteOutcome::Complete
        }
    }
    impl StdinTransport for Transport {
        fn close(&mut self) {}
    }
    for (rows, cancelled) in [
        (
            json!(vec![json!({"sessionId":SESSION,"title":"Fix tests"}); 513]),
            false,
        ),
        (
            json!([{"sessionId":SESSION,"title":"x".repeat(1_048_576)}]),
            false,
        ),
        (
            json!(vec![json!({"sessionId":SESSION,"title":"Fix tests"}); 513]),
            true,
        ),
        (
            json!([{"sessionId":SESSION,"title":"x".repeat(1_048_576)}]),
            true,
        ),
    ] {
        let mut fixture = running("oversized-title-pump");
        fixture.runtime.start_title("").unwrap();
        let request = poll(&mut fixture.runtime);
        if cancelled {
            fixture.runtime.cancel_title();
        }
        let owner = RuntimeOwner {
            project_id: SageProjectId::parse("project-title").unwrap(),
            chat_id: SageChatId::parse("chat-title").unwrap(),
            session_id: SageAgentSessionId::parse("session-title").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let session = ActivatedSession {
            binding_digest: [1; 32],
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            external_session_id: ExternalId::new("session", SESSION).unwrap(),
            capabilities: fixture.runtime.capabilities().unwrap(),
            models: Vec::new(),
            next_model_cursor: None,
        };
        let replacement = NativeRuntime::new(crate::acp::bindings::codex::CodexRuntime::fixture(
            crate::acp::bindings::codex::Platform::WindowsX86_64,
        ));
        let runtime = std::mem::replace(&mut fixture.runtime, replacement);
        let mut pump = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(runtime, (), Mapper, session),
            idle_decoder: IncrementalFrameDecoder::new(),
            transport: Transport,
            control: (),
            owner,
            clock: Arc::new(SystemDriverClock),
        };
        let mut bytes =
            serde_json::to_vec(&json!({"id":request["id"],"result":{"sessions":rows}})).unwrap();
        bytes.push(b'\n');
        for chunk in bytes.chunks(8192) {
            pump.accept_idle_stdout_chunk(chunk).unwrap();
        }
        let (mut runtime, _, _, _) = pump.parts.into_parts();
        assert_eq!(
            runtime.take_title_result(),
            if cancelled { None } else { Some(None) }
        );
        assert!(!runtime.title_pending());
        assert!(
            runtime
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "Continue").unwrap(),
                    client_message_id: RequiredText::new("client", "foreground").unwrap(),
                    access_mode: SessionAccessMode::ReadOnly,
                })
                .is_ok()
        );
    }
}

#[test]
fn selects_only_the_bound_id_and_rereads_a_default_once_without_paging() {
    let mut fixture = running("read-title");
    fixture.runtime.start_title("ignored").unwrap();
    let first = poll(&mut fixture.runtime);
    assert_eq!(first["method"], "session/list");
    assert!(first["params"]["cwd"].is_string());
    assert!(first["params"].get("cursor").is_none());
    let reply = |id: &Value, title: &str| {
        json!({"jsonrpc":"2.0","id":id,"result":{
        "sessions":[{"sessionId":"foreign","title":"Never use this title"},
            {"sessionId":SESSION,"title":title}],"nextCursor":"do-not-follow"}})
    };
    let batch = fixture
        .runtime
        .ingest(
            &frame(reply(
                &first["id"],
                "New session - 2026-11-21T12:00:00.000Z",
            )),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [crate::acp::bindings::adapter::AdapterEvent::Absorbed]
    ));
    assert_eq!(fixture.runtime.take_title_result(), None);
    assert!(fixture.runtime.poll_title().is_none());
    std::thread::sleep(std::time::Duration::from_millis(510));
    let second = poll(&mut fixture.runtime);
    fixture
        .runtime
        .ingest(&frame(reply(&first["id"], "Untitled")), &Mapper)
        .unwrap();
    assert_eq!(
        fixture.runtime.take_title_result(),
        None,
        "a duplicate first page cannot consume the reread"
    );
    assert!(fixture.runtime.title_pending());
    fixture
        .runtime
        .ingest(
            &frame(reply(&second["id"], "Investigate empty sidebar chats")),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        fixture.runtime.take_title_result(),
        Some(Some("Investigate empty sidebar chats".into()))
    );
    assert!(!fixture.runtime.title_pending());
    assert!(fixture.runtime.poll_title().is_none());
}

#[test]
fn a_second_default_absent_id_or_oversized_page_keeps_the_local_title() {
    for rows in [
        json!([]),
        json!([{"sessionId":"foreign","title":"Useful but foreign title"}]),
        json!([{"sessionId":SESSION,"title":"Untitled"}]),
        json!(vec![
            json!({"sessionId":SESSION,"title":"Useful bounded title"});
            101
        ]),
    ] {
        let mut fixture = running("read-title-fallback");
        fixture.runtime.start_title("").unwrap();
        for _ in 0..2 {
            if !fixture.runtime.title_pending() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(510));
            let request = poll(&mut fixture.runtime);
            fixture
                .runtime
                .ingest(
                    &frame(json!({"id":request["id"],"result":{"sessions":rows}})),
                    &Mapper,
                )
                .unwrap();
        }
        assert_eq!(fixture.runtime.take_title_result(), Some(None));
        assert!(fixture.runtime.poll_title().is_none());
    }
}

#[test]
fn cancelled_and_failed_reads_absorb_late_results_and_release_capacity() {
    let mut fixture = running("cancel-title");
    fixture.runtime.start_title("").unwrap();
    let request = poll(&mut fixture.runtime);
    fixture.runtime.cancel_title();
    fixture.runtime.ingest(&frame(json!({"id":request["id"],"result":{"sessions":[{"sessionId":SESSION,"title":"A late generated title"}]}})), &Mapper).unwrap();
    assert!(!fixture.runtime.title_pending());
    assert_eq!(fixture.runtime.take_title_result(), None);
}

#[test]
fn partially_written_title_read_blocks_foreground_commands() {
    use crate::acp::runtime::EgressTransport;
    use crate::acp::transport::{FrameWriteOutcome, OutboundFrame};
    struct PartialTransport;
    impl EgressTransport for PartialTransport {
        fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 7 }
        }
    }
    let mut fixture = running("partial-title");
    fixture.runtime.start_title("").unwrap();
    let egress = fixture.runtime.poll_title().unwrap();
    let outcome = PartialTransport.write(egress.frame());
    fixture
        .runtime
        .resolve_egress(egress.into_token(), EgressResolution::from(outcome))
        .unwrap();
    assert!(fixture.runtime.title_requires_teardown());
    assert!(
        fixture
            .runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Continue").unwrap(),
                client_message_id: RequiredText::new("client", "foreground").unwrap(),
                access_mode: SessionAccessMode::ReadOnly,
            })
            .is_err()
    );
}
