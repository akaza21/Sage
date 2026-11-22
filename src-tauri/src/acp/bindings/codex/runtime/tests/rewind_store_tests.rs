use super::*;
use crate::acp::{goal::NativeRpcReply, store::rewind_tests::history};
use sage_acp_protocol::{scheduler::RuntimeOwner, session_control::SessionControl};

#[test]
fn admitted_prompts_resolve_the_native_turn_and_settle_the_same_durable_suffix() {
    for prompt in [
        Prompt::new("prompt", "Repeat").unwrap(),
        history::quoted_prompt(),
        history::file_prompt(),
    ] {
        // Build the native content with the production turn/start builder.
        let mut sender = fixture_runtime();
        ready(&mut sender);
        let sent = sender
            .command(AdapterCommand::Prompt {
                prompt: prompt.clone(),
                client_message_id: RequiredText::new("client", "sent-prompt").unwrap(),
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let sent: Value = serde_json::from_slice(sent.frame().payload()).unwrap();
        let content = sent["params"]["input"].clone();
        assert!(content.is_array());

        let mut history = history::History::new();
        history.turn(prompt.clone(), None);
        // Repeated text makes an off-by-one select the wrong turn even when
        // its digest matches. Accepted steering belongs to that same turn.
        history.turn(prompt, Some("Steer the second turn"));
        history.unwritten(Prompt::new("prompt", "Unwritten B").unwrap());
        let target = history.target(0).unwrap();
        assert_eq!(target.following_messages, 2);
        assert!(target.native_turn_id.is_none());
        let control = SessionControl::RewindConversation { before: target };
        let fixture = &history.fixture;
        let command = crate::acp::turn_undo::stage_turn_undo(
            &fixture.database,
            &RuntimeOwner {
                project_id: fixture.project.clone(),
                chat_id: fixture.chat.clone(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
            },
            history.turns[0].clone(),
            true,
            true,
            || Ok::<_, String>(None),
        )
        .unwrap();
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        let read = runtime.session_control(&control).unwrap();
        let id = request_id(&read);
        runtime
            .resolve_egress(read.into_token(), EgressResolution::Written)
            .unwrap();
        let batch = runtime
            .ingest(
                &frame(json!({"id":id,"result":{"data":[
            {"id":"native-second","status":"completed","items":[
                {"type":"userMessage","content":content},
                {"type":"userMessage","content":[{"type":"text","text":"Steer the second turn"}]}
            ]},
            {"id":"native-first","status":"completed","items":[
                {"type":"userMessage","content":content}
            ]}
        ],"nextCursor":null}})),
                &Mapper,
            )
            .unwrap();
        let (_, mut egress) = batch.into_parts();
        let revert = egress
            .pop()
            .expect("the verified target must produce a native revert");
        let request: Value = serde_json::from_slice(revert.frame().payload()).unwrap();
        assert_eq!(request["method"], "thread/revert");
        assert_eq!(
            request["params"],
            json!({"threadId":"thread-1","beforeTurnId":"native-first"})
        );
        runtime
            .resolve_egress(revert.into_token(), EgressResolution::Written)
            .unwrap();
        runtime
            .ingest(
                &frame(json!({"id":request["id"],"result":{
                    "thread":{"id":"thread-1","turns":[]}
                }})),
                &Mapper,
            )
            .unwrap();
        assert_eq!(
            runtime.take_native_rpc_reply(),
            Some(NativeRpcReply::Controlled(None))
        );
        crate::acp::store::record_turn_undone(&mut fixture.database.connection(), &command)
            .unwrap();
        assert!(fixture.page_turns().is_empty());
    }
}
