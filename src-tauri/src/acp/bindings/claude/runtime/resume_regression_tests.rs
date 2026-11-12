use super::*;
use crate::acp::contract::{ProjectionRevision, SageChatId, SageProjectId};
use crate::acp::runtime::{
    ActivationIntent, ActivationJournal, ActivationJournalError, ActivationMode, ActivationPump,
    ActivationReceipt, ActivationTransport,
};
use crate::acp::store::CompleteActivationCommand;
use crate::acp::transport::{FrameWriteOutcome, OutboundFrame};

struct Journal;
impl ActivationJournal for Journal {
    fn commit_ready<'a>(
        &'a mut self,
        _: CompleteActivationCommand,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ActivationReceipt, ActivationJournalError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(1).unwrap(),
            })
        })
    }
}
struct Sink;
impl ActivationTransport for Sink {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        FrameWriteOutcome::Complete
    }
}

#[test]
fn a_fresh_runtime_resumes_the_stopped_chat_through_activation_and_sends_again() {
    let mut prior = ClaudeRuntime::fixture();
    ready(&mut prior);
    let ceiling = prior.capabilities().unwrap();
    let (id, token) = prompt(&mut prior);
    prior
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let cancel = prior.command(AdapterCommand::Cancel).unwrap();
    prior
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();
    prior
        .ingest(
            &frame(json!({"id":id,"result":{"stopReason":"cancelled"}})),
            &Mapper,
        )
        .unwrap();
    drop(prior);

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = ActivationIntent {
            binding_digest: [7;32], artifact_digest: "d".repeat(64),
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
            mode: ActivationMode::Resume {
                external_session_id: ExternalId::new("external", "claude-session-1").unwrap(),
                native_root: r"C:\fixture\root".into(), model:None, rewind_at: None,
            }, curated_ceiling: ceiling.clone(), sage_policy: ceiling,
        };
        let mut pump = ActivationPump::new(ClaudeRuntime::fixture(), Journal, Mapper, intent).unwrap();
        pump.begin().unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let initialize = json!({"jsonrpc":"2.0","id":1,"result":{
            "protocolVersion":1,"agentCapabilities":{"sessionCapabilities":{"resume":{},"close":{}}},
            "models":[{"id":"claude-reviewed","name":"Claude reviewed"}]
        }});
        pump.push_stdout_chunk(format!("{initialize}\n").as_bytes(), 1).await.unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let resumed = json!({"jsonrpc":"2.0","id":2,"result":{"sessionId":"claude-session-1",
            "modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}}});
        pump.push_stdout_chunk(format!("{resumed}\n").as_bytes(), 2).await.expect("resume must cross ReadyHandoff");
        let parts = pump.try_into_idle_parts().ok().expect("reusable after cold resume");
        let (mut runtime, _, _, _) = parts.into_parts();
        let next = runtime.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt","Continue").unwrap(),
            client_message_id: RequiredText::new("client","retry").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::Ask,
        }).unwrap();
        assert_eq!(request_id(&next), 3);
    });
}
