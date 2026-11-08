use serde_json::{Value, json};

use super::*;

const SESSION: &str = "0b7d6c8e-3c1a-4f7e-9a51-2f6d8c1e4b90";

fn initialized() -> SdkBridge {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    let sdk = bridge
        .outbound(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .unwrap();
    assert_eq!(sdk["request"]["subtype"], "initialize");
    let reply = json!({
        "commands": [{"name":"compact","description":"Compact the chat","argumentHint":"[focus]"}],
        "current_permission_mode": "default",
        "models": [{"value":"default","displayName":"Default","supportsEffort":true,"supportedEffortLevels":["low","high"]}],
    });
    let acp = bridge.inbound(success(&sdk, reply)).unwrap().acp;
    assert_eq!(
        acp[0]["result"]["agentCapabilities"]["sessionCapabilities"],
        json!({"resume":{}})
    );
    let sdk = bridge.outbound(&json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/w","mcpServers":[]}})).unwrap();
    assert_eq!(sdk["request"]["subtype"], "get_settings");
    let acp = bridge
        .inbound(success(&sdk, json!({"applied":{}})))
        .unwrap()
        .acp;
    assert_eq!(acp[0]["result"]["sessionId"], SESSION);
    assert_eq!(
        acp[1]["params"]["update"]["availableCommands"],
        json!([{"name":"compact","description":"Compact the chat","input":{"hint":"[focus]"}}])
    );
    bridge
}

fn success(request: &Value, response: Value) -> Value {
    json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":response}})
}

fn updates(inbound: &Inbound) -> Vec<&Value> {
    inbound
        .acp
        .iter()
        .map(|frame| &frame["params"]["update"])
        .collect()
}

fn prompt(bridge: &mut SdkBridge, id: u64) -> Value {
    bridge
        .outbound(&json!({"jsonrpc":"2.0","id":id,"method":"session/prompt","params":{"sessionId":SESSION,"prompt":[{"type":"text","text":"hi"}]}}))
        .unwrap()
}

#[test]
fn a_turn_streams_text_runs_an_approved_tool_and_completes_with_usage() {
    let mut bridge = initialized();
    let user = prompt(&mut bridge, 3);
    assert_eq!(
        user["message"]["content"],
        json!([{"type":"text","text":"hi"}])
    );
    assert_eq!(user["session_id"], SESSION);

    bridge.inbound(json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start","message":{"id":"msg_1"}}})).unwrap();
    let chunk = bridge.inbound(json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Writing"}}})).unwrap();
    assert_eq!(
        updates(&chunk),
        [
            &json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Writing"}})
        ]
    );

    let assistant = bridge.inbound(json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[
        {"type":"text","text":"Writing"},
        {"type":"tool_use","id":"toolu_1","name":"Write","input":{"file_path":"/w/a.txt","content":"x\n"}}
    ]}})).unwrap();
    let tool = updates(&assistant);
    assert_eq!(tool.len(), 1, "streamed text is not repeated");
    assert_eq!(
        (
            tool[0]["sessionUpdate"].as_str(),
            tool[0]["toolCallId"].as_str(),
            tool[0]["kind"].as_str()
        ),
        (Some("tool_call"), Some("toolu_1"), Some("edit"))
    );

    let ask = bridge.inbound(json!({"type":"control_request","request_id":"cli-7","request":{
        "subtype":"can_use_tool","tool_name":"Write","tool_use_id":"toolu_1",
        "input":{"file_path":"/w/a.txt","content":"x\n"},
        "permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]
    }})).unwrap();
    let request = &ask.acp[0];
    assert_eq!(request["method"], "session/request_permission");
    assert_eq!(request["params"]["toolCall"]["toolCallId"], "toolu_1");
    let kinds: Vec<_> = request["params"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["allow_once", "allow_always", "reject_once"]);

    let reply = bridge.outbound(&json!({"jsonrpc":"2.0","id":request["id"],"result":{"outcome":{"outcome":"selected","optionId":"allow_always"}}})).unwrap();
    assert_eq!(reply["response"]["request_id"], "cli-7");
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["file_path"],
        "/w/a.txt"
    );
    assert_eq!(
        reply["response"]["response"]["updatedPermissions"][0]["mode"],
        "acceptEdits"
    );

    let done = bridge
        .inbound(
            json!({"type":"user","parent_tool_use_id":null,"message":{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_1","content":"File created"}
            ]}}),
        )
        .unwrap();
    let update = updates(&done)[0];
    assert_eq!(
        (update["status"].as_str(), update.get("content")),
        (Some("completed"), None),
        "an edit keeps its diff"
    );

    let result = bridge.inbound(json!({"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn",
        "usage":{"input_tokens":2,"output_tokens":4,"cache_read_input_tokens":100,"cache_creation_input_tokens":0},
        "modelUsage":{"claude-sonnet-5-5":{"contextWindow":200000}}})).unwrap();
    assert_eq!(
        updates(&result)[0],
        &json!({"sessionUpdate":"usage_update","used":106,"size":200000})
    );
    assert_eq!(result.acp[1]["id"], 3);
    assert_eq!(result.acp[1]["result"]["stopReason"], "end_turn");
    assert_eq!(result.acp[1]["result"]["usage"]["totalTokens"], 106);
}

#[test]
fn a_rejected_tool_is_denied_and_its_failure_text_is_shown() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    bridge
        .inbound(
            json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
        {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf build"}}]}}),
        )
        .unwrap();
    let ask = bridge.inbound(json!({"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"rm -rf build"}}})).unwrap();
    let options = ask.acp[0]["params"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 2, "no always-allow without suggested rules");
    let reply = bridge.outbound(&json!({"jsonrpc":"2.0","id":ask.acp[0]["id"],"result":{"outcome":{"outcome":"cancelled"}}})).unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "deny");
    let failed = bridge.inbound(json!({"type":"user","parent_tool_use_id":null,"message":{"content":[
        {"type":"tool_result","tool_use_id":"t1","is_error":true,"content":[{"type":"text","text":"Permission denied"}]}]}})).unwrap();
    let update = updates(&failed)[0];
    assert_eq!(update["status"], "failed");
    assert_eq!(update["content"][0]["content"]["text"], "Permission denied");
}

#[test]
fn an_interrupted_turn_ends_cancelled_even_when_claude_reports_an_error() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let interrupt = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    assert!(
        bridge
            .inbound(success(&interrupt, json!({})))
            .unwrap()
            .acp
            .is_empty()
    );
    let result = bridge
        .inbound(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}),
        )
        .unwrap();
    assert_eq!(result.acp[0]["result"]["stopReason"], "cancelled");
}

#[test]
fn a_sign_in_failure_ends_the_turn_with_the_auth_required_error() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let assistant = bridge.inbound(json!({"type":"assistant","parent_tool_use_id":null,"error":"authentication_failed",
        "message":{"id":"synthetic","content":[{"type":"text","text":"Not logged in · Please run /login"}]}})).unwrap();
    assert_eq!(
        updates(&assistant)[0]["content"]["text"],
        "Not logged in · Please run /login"
    );
    let result = bridge.inbound(json!({"type":"result","subtype":"success","is_error":true,"result":"Not logged in","usage":{}})).unwrap();
    assert_eq!(
        result.acp[0]["error"],
        json!({"code":-32000,"message":"Authentication required"})
    );
}

#[test]
fn todo_write_replaces_the_plan_instead_of_adding_a_tool_row() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let plan = bridge.inbound(json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
        {"type":"tool_use","id":"t","name":"TodoWrite","input":{"todos":[{"content":"Ship","activeForm":"Shipping","status":"in_progress"}]}}]}})).unwrap();
    assert_eq!(
        updates(&plan)[0],
        &json!({"sessionUpdate":"plan","entries":[{"content":"Shipping","status":"in_progress","priority":"medium"}]})
    );
    let result = bridge.inbound(json!({"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"ok"}]}})).unwrap();
    assert!(result.acp.is_empty());
}

#[test]
fn effort_changes_apply_as_a_flag_setting_and_echo_the_config() {
    let mut bridge = initialized();
    let sdk = bridge.outbound(&json!({"jsonrpc":"2.0","id":4,"method":"session/set_config_option","params":{"sessionId":SESSION,"configId":"effort","value":"high"}})).unwrap();
    assert_eq!(
        sdk["request"],
        json!({"subtype":"apply_flag_settings","settings":{"effortLevel":"high"}})
    );
    let acp = bridge.inbound(success(&sdk, json!({}))).unwrap().acp;
    let effort = acp[0]["result"]["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "effort")
        .unwrap();
    assert_eq!(effort["currentValue"], "high");
    assert_eq!(
        bridge.outbound(&json!({"jsonrpc":"2.0","id":5,"method":"session/set_config_option","params":{"configId":"effort","value":"turbo"}})),
        Err(BridgeError::Unsupported)
    );
}

#[test]
fn subagent_traffic_and_unserved_control_requests_stay_out_of_the_parent_transcript() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let nested = bridge.inbound(json!({"type":"stream_event","parent_tool_use_id":"toolu_task","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"inner"}}})).unwrap();
    assert_eq!(nested, Inbound::default());
    let hook = bridge.inbound(json!({"type":"control_request","request_id":"h1","request":{"subtype":"hook_callback","callback_id":"x"}})).unwrap();
    assert!(hook.acp.is_empty());
    assert_eq!(hook.replies[0]["response"]["subtype"], "error");
    assert_eq!(hook.replies[0]["response"]["request_id"], "h1");
}

#[test]
fn attachments_become_text_image_and_document_content() {
    let mut bridge = initialized();
    let blob = |name: &str, data: &str| {
        json!({"type":"resource","resource":{
            "uri": format!("sage-attachment:///a/{name}"), "mimeType":"application/octet-stream", "blob": data,
        }})
    };
    let user = bridge.outbound(&json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"prompt":[
        {"type":"text","text":"see"},
        {"type":"resource","resource":{"uri":"sage-attachment:///a/notes.md","mimeType":"text/plain","text":"# Notes"}},
        blob("shot.png", "iVBORw0KGgoAAAANSUhEUg=="),
        blob("spec.pdf", "JVBERi0xLjcKJeLjz9MK"),
        blob("bundle.zip", "UEsDBBQAAAAIAGFiY2RlZg=="),
    ]}})).unwrap();
    let content = &user["message"]["content"];
    assert_eq!(
        content[1]["text"],
        "<context ref=\"sage-attachment:///a/notes.md\">\n# Notes\n</context>"
    );
    assert_eq!(
        content[2],
        json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUg=="}})
    );
    assert_eq!(content[3]["type"], "document");
    assert_eq!(content[3]["source"]["media_type"], "application/pdf");
    assert_eq!(content[4]["type"], "text");
    assert!(content[4]["text"].as_str().unwrap().contains("bundle.zip"));
}

#[test]
fn resume_must_name_the_launched_session() {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    assert_eq!(
        bridge.outbound(&json!({"jsonrpc":"2.0","id":2,"method":"session/resume","params":{"sessionId":"another"}})),
        Err(BridgeError::Unsupported)
    );
}

#[test]
fn a_mode_claude_code_switches_to_itself_reaches_the_picker_once() {
    let mut bridge = initialized();
    let status = |mode: &str| json!({"type":"system","subtype":"status","status":null,"permissionMode":mode});
    let acp = bridge.inbound(status("acceptEdits")).unwrap().acp;
    assert_eq!(
        acp[0]["params"]["update"],
        json!({"sessionUpdate":"current_mode_update","currentModeId":"acceptEdits"})
    );
    assert!(
        bridge
            .inbound(status("acceptEdits"))
            .unwrap()
            .acp
            .is_empty()
    );
    assert!(
        bridge
            .inbound(status("bypassPermissions"))
            .unwrap()
            .acp
            .is_empty()
    );
}

#[test]
fn withdrawn_and_outlived_approvals_cancel_their_cards() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let ask = |bridge: &mut SdkBridge, request: &str| {
        let asked = bridge.inbound(json!({"type":"control_request","request_id":request,"request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}})).unwrap();
        assert_eq!(
            asked.acp[0]["params"]["update"]["sessionUpdate"],
            "tool_call"
        );
        asked.acp[1]["id"].as_u64().unwrap()
    };
    let withdrawn = ask(&mut bridge, "r1");
    let outlived = ask(&mut bridge, "r2");
    let cancel = bridge
        .inbound(json!({"type":"control_cancel_request","request_id":"r1"}))
        .unwrap();
    assert_eq!(cancel.withdrawn, [withdrawn]);
    let ended = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false}))
        .unwrap();
    assert_eq!(ended.withdrawn, [outlived]);
    assert!(bridge.permissions.is_empty());
    let late = bridge.outbound(
        &json!({"id":outlived,"result":{"outcome":{"outcome":"selected","optionId":"allow_once"}}}),
    );
    assert_eq!(late, Err(BridgeError::Unsupported));
    let after = bridge.inbound(json!({"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}})).unwrap();
    assert!(after.acp.is_empty(), "no turn is running");
    assert_eq!(after.replies[0]["response"]["response"]["behavior"], "deny");
}

#[test]
fn an_approval_before_its_tool_use_shows_one_row() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    bridge.inbound(json!({"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"ls"}}})).unwrap();
    let assistant = bridge
        .inbound(json!({"type":"assistant","message":{"id":"m","content":[
            {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}]}}))
        .unwrap();
    assert!(assistant.acp.is_empty());
}

#[test]
fn an_interrupt_claude_code_never_received_does_not_cancel_the_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let interrupt = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    bridge.undo(&SdkBridge::sent(&interrupt).unwrap());
    let ended = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false}))
        .unwrap();
    assert_eq!(
        ended.acp.last().unwrap()["result"]["stopReason"],
        "end_turn"
    );
}

#[test]
fn a_reply_too_large_to_read_fails_its_request() {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    let sdk = bridge
        .outbound(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .unwrap();
    let kept = SdkBridge::essentials(&success(&sdk, json!({"commands": []}))).unwrap();
    let acp = bridge.inbound(kept).unwrap().acp;
    assert_eq!(acp[0]["id"], 1);
    assert!(acp[0]["error"].is_object());
}

#[test]
fn questions_are_declined_so_claude_asks_in_its_reply() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let asked = bridge.inbound(json!({"type":"control_request","request_id":"q1","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[]}}})).unwrap();
    assert!(asked.acp.is_empty());
    assert_eq!(asked.replies[0]["response"]["response"]["behavior"], "deny");
}

#[test]
fn a_turn_ends_tool_rows_claude_code_never_finished_as_failed() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    bridge
        .inbound(json!({"type":"assistant","message":{"id":"m","content":[
            {"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/w/big.png"}}]}}))
        .unwrap();
    let ended = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false}))
        .unwrap();
    let update = updates(&ended)[0];
    assert_eq!(
        (update["toolCallId"].as_str(), update["status"].as_str()),
        (Some("t1"), Some("failed"))
    );
}

#[test]
fn a_cli_that_starts_outside_the_picker_modes_fails_initialize() {
    let mut bridge = SdkBridge::new(SESSION.into(), None);
    let sdk = bridge
        .outbound(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}))
        .unwrap();
    let acp = bridge
        .inbound(success(
            &sdk,
            json!({"current_permission_mode":"bypassPermissions"}),
        ))
        .unwrap()
        .acp;
    assert!(
        acp[0]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("permission mode")
    );
}

#[test]
fn claude_codes_status_messages_become_notices_and_activities() {
    use crate::acp::intent::session_event::{SessionActivityState, SessionScopedEvent};
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let state = |inbound: &Inbound| match &inbound.session[..] {
        [SessionScopedEvent::Activity(activity)] => {
            (activity.kind.as_str().to_owned(), activity.state)
        }
        other => panic!("one activity: {other:?}"),
    };
    let compacting = bridge
        .inbound(json!({"type":"system","subtype":"status","status":"compacting"}))
        .unwrap();
    assert_eq!(
        state(&compacting),
        ("compaction".into(), SessionActivityState::Running)
    );
    let done = bridge
        .inbound(
            json!({"type":"system","subtype":"status","status":null,"compact_result":"success"}),
        )
        .unwrap();
    assert_eq!(
        state(&done),
        ("compaction".into(), SessionActivityState::Completed)
    );
    let boundary = bridge.inbound(json!({"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"auto","pre_tokens":9000,"post_tokens":1200}})).unwrap();
    let [SessionScopedEvent::Notice(notice)] = &boundary.session[..] else {
        panic!("notice")
    };
    assert!(notice.text.as_str().contains("9000 to 1200"));

    let task = bridge.inbound(json!({"type":"system","subtype":"task_started","task_id":"b1","description":"Run the test suite"})).unwrap();
    assert_eq!(state(&task), ("task".into(), SessionActivityState::Running));
    let finished = bridge.inbound(json!({"type":"system","subtype":"task_notification","task_id":"b1","status":"completed","output_file":"/tmp/x","summary":"All tests passed"})).unwrap();
    assert_eq!(
        state(&finished),
        ("task".into(), SessionActivityState::Completed)
    );

    let limit = bridge.inbound(json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"five_hour","utilization":0.91}})).unwrap();
    let [SessionScopedEvent::Notice(notice)] = &limit.session[..] else {
        panic!("notice")
    };
    assert!(notice.text.as_str().contains("5-hour limit (91% used)"));
    let quiet = bridge
        .inbound(json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}))
        .unwrap();
    assert!(quiet.session.is_empty());

    bridge.inbound(json!({"type":"system","subtype":"api_retry","attempt":2,"max_retries":10,"retry_delay_ms":500,"error_status":529,"error":"overloaded"})).unwrap();
    let ended = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false}))
        .unwrap();
    assert_eq!(
        state(&ended),
        ("retry".into(), SessionActivityState::Completed)
    );
}

#[test]
fn local_command_output_and_new_commands_reach_the_chat() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let output = bridge
        .inbound(
            json!({"type":"system","subtype":"local_command_output","content":"Total cost: $0.12"}),
        )
        .unwrap();
    assert_eq!(updates(&output)[0]["content"]["text"], "Total cost: $0.12");
    let commands = bridge.inbound(json!({"type":"system","subtype":"commands_changed","commands":[{"name":"deploy","description":"Ship it","argumentHint":""}]})).unwrap();
    assert_eq!(
        updates(&commands)[0]["availableCommands"][0]["name"],
        "deploy"
    );
}
