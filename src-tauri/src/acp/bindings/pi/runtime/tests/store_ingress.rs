use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId, SageTurnId},
    store::{ItemIngressCommand, apply_item_ingress},
};
use rusqlite::params;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn assistant_message(content: Value, stop_reason: &str) -> Value {
    json!({"role":"assistant","content":content,"stopReason":stop_reason,
        "api":"openai-responses","provider":"openai","model":"fixture-model","timestamp":0,
        "usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}}})
}

#[test]
fn pi_consecutive_turn_items_and_images_ingest_without_identity_conflicts() {
    let mut connection = crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:"))
        .expect("migrated store");
    let mut runtime = fixture_runtime(71);
    ready(&mut runtime);
    connection
        .execute_batch(
            "INSERT INTO projects VALUES('project-pi','Pi',0,0,0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-pi','project-pi',0,'Pi',0,0,0);
         INSERT INTO acp_project_usage VALUES('project-pi',1,1,2,0,0,0,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=2;",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
         capabilities_json,state,residency,created_at,updated_at)
         VALUES(?1,'project-pi','chat-pi',0,'pi',?2,'{}','active','hot',0,0)",
            params![runtime.session_id.as_str(), "07".repeat(32)],
        )
        .unwrap();
    connection.execute(
        "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
         VALUES('project-pi','chat-pi',?1,71,'running',0)",
        [runtime.session_id.as_str()],
    ).unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage VALUES('project-pi','chat-pi',?1,2,0,0,0,0,0,0)",
            [runtime.session_id.as_str()],
        )
        .unwrap();
    let mut completed = Vec::new();
    let mut images = Vec::new();
    for ordinal in 0..2 {
        let client = format!("client-store-{ordinal}");
        let turn_id = SageTurnId::parse(format!("turn-store-{ordinal}")).unwrap();
        connection
            .execute(
                "INSERT INTO turns VALUES(?1,'project-pi','chat-pi',?2,71,?3,?4,
             'running','correlated',0,0,0,NULL)",
                params![
                    turn_id.as_str(),
                    runtime.session_id.as_str(),
                    ordinal,
                    client
                ],
            )
            .unwrap();
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Inspect read-only").unwrap(),
                client_message_id: RequiredText::new("client", client.as_str()).unwrap(),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let prompt = deliver(&mut runtime, prompt);
        runtime
            .ingest(&response(&prompt, Value::Null), &Mapper)
            .unwrap();
        let tool_id = format!("read-store-{ordinal}");
        let assistant = assistant_message(
            json!([
                {"type":"thinking","thinking":"Inspecting"},
                {"type":"text","text":"Inspecting image"},
                {"type":"toolCall","id":tool_id,"name":"read","arguments":{"path":"fixture.png"}}
            ]),
            "toolUse",
        );
        let thought_start = assistant_message(json!([{"type":"thinking","thinking":""}]), "stop");
        let thought =
            assistant_message(json!([{"type":"thinking","thinking":"Inspecting"}]), "stop");
        let text = assistant_message(
            json!([
                {"type":"thinking","thinking":"Inspecting"},
                {"type":"text","text":"Inspecting image"}
            ]),
            "stop",
        );
        let user = json!({"role":"user","content":[{"type":"text","text":"Inspect read-only"}],"timestamp":0});
        let image = json!({
            "type":"image","mimeType":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/xioAAAAASUVORK5CYII="
        });
        let tool_result = json!({"role":"toolResult","toolCallId":tool_id,"toolName":"read",
            "content":[image],"isError":false,"timestamp":1});
        let followup = assistant_message(json!([{"type":"text","text":"Done"}]), "stop");
        let empty_assistant = assistant_message(json!([]), "stop");
        let events = [
            json!({"type":"agent_start"}),
            json!({"type":"turn_start"}),
            json!({"type":"message_start","message":user}),
            json!({"type":"message_end","message":user}),
            json!({"type":"message_start","message":empty_assistant}),
            json!({"type":"message_update","message":thought_start,"assistantMessageEvent":{
                "type":"thinking_start","contentIndex":0,"partial":thought_start}}),
            json!({"type":"message_update","message":thought,"assistantMessageEvent":{
                "type":"thinking_delta","contentIndex":0,"delta":"Inspecting","partial":thought}}),
            json!({"type":"message_update","message":thought,"assistantMessageEvent":{
                "type":"thinking_end","contentIndex":0,"content":"Inspecting","partial":thought}}),
            json!({"type":"message_update","message":text,"assistantMessageEvent":{
                "type":"text_delta","contentIndex":1,"delta":"Inspecting image","partial":text}}),
            json!({"type":"message_end","message":assistant}),
            json!({"type":"tool_execution_start","toolCallId":tool_id,"toolName":"read","args":{"path":"fixture.png"}}),
            json!({"type":"tool_execution_end","toolCallId":tool_id,"toolName":"read","result":{"content":[image]},"isError":false}),
            json!({"type":"message_start","message":tool_result}),
            json!({"type":"message_end","message":tool_result}),
            json!({"type":"turn_end","message":assistant,"toolResults":[tool_result]}),
            json!({"type":"turn_start"}),
            json!({"type":"message_start","message":empty_assistant}),
            json!({"type":"message_update","message":followup,"assistantMessageEvent":{
                "type":"text_delta","contentIndex":0,"delta":"Done","partial":followup}}),
            json!({"type":"message_end","message":followup}),
            json!({"type":"turn_end","message":followup,"toolResults":[]}),
            json!({"type":"agent_end","messages":[user,assistant,tool_result,followup],"willRetry":false}),
            json!({"type":"compaction_start","reason":"threshold"}),
            json!({"type":"compaction_end","reason":"threshold","aborted":true,"willRetry":false}),
            json!({"type":"agent_settled"}),
        ];
        let mut kinds = BTreeSet::new();
        for value in events {
            let batch = runtime.ingest(&frame(value), &Mapper).expect("Pi event");
            for event in batch.events() {
                if let AdapterEvent::Item {
                    id,
                    lifecycle,
                    tool_state,
                    payload,
                } = event
                {
                    assert_eq!(id.turn_client_message_id.as_ref().unwrap().as_str(), client);
                    let bytes = serde_json::to_vec(payload).unwrap();
                    let digest = format!("{:x}", Sha256::digest(bytes));
                    let mutation = apply_item_ingress(
                        &mut connection,
                        ItemIngressCommand {
                            project_id: SageProjectId::parse("project-pi").unwrap(),
                            chat_id: SageChatId::parse("chat-pi").unwrap(),
                            session_id: id.session_id.clone(),
                            turn_id: turn_id.clone(),
                            runtime_epoch: id.runtime_epoch,
                            external_item_id: id.external_id.clone(),
                            activity_id: None,
                            payload: payload.clone(),
                            payload_digest: digest,
                            lifecycle: *lifecycle,
                            tool_state: *tool_state,
                            now_millis: 1,
                        },
                    )
                    .expect("distinct durable item identity across turns");
                    if let NormalizedPayload::Image { .. } = payload {
                        assert_eq!(*lifecycle, crate::acp::lifecycle::ItemLifecycle::Completed);
                        images.push(mutation.item_id.as_str().to_owned());
                    }
                    if lifecycle.is_terminal()
                        && matches!(
                            payload,
                            NormalizedPayload::Text { .. }
                                | NormalizedPayload::ThoughtSummary { .. }
                                | NormalizedPayload::CompactionNotice
                                | NormalizedPayload::Image { .. }
                        )
                    {
                        kinds.insert(payload.kind().as_str());
                        completed.push(mutation.item_id.as_str().to_owned());
                    }
                }
            }
        }
        assert_eq!(
            kinds,
            BTreeSet::from(["text", "thoughtSummary", "compactionNotice", "image"])
        );
        connection
            .execute(
                "UPDATE turns SET state='completed',terminal_at=2 WHERE id=?1",
                [turn_id.as_str()],
            )
            .unwrap();
    }
    assert_eq!(completed.len(), 10);
    assert_eq!(completed.into_iter().collect::<BTreeSet<_>>().len(), 10);
    assert_eq!(images.len(), 2);
    assert_eq!(images.into_iter().collect::<BTreeSet<_>>().len(), 2);
}
