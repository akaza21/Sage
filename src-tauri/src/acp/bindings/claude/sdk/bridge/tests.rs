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
fn a_linked_file_becomes_a_mention_claude_code_reads() {
    let mut bridge = initialized();
    let (uri, path) = if cfg!(windows) {
        (
            "file:///C:/work/my%20notes.md",
            "@\"C:\\work\\my notes.md\"",
        )
    } else {
        ("file:///work/my%20notes.md", "@\"/work/my notes.md\"")
    };
    let user = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"prompt":[
                {"type":"text","text":"see"},
                {"type":"resource_link","uri":uri,"name":"my notes.md"},
            ]}}),
        )
        .unwrap();
    assert_eq!(user["message"]["content"][1]["text"], path);
    assert_eq!(
        bridge.outbound(
            &json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"prompt":[
                {"type":"resource_link","uri":"https://example.com/x","name":"x"},
            ]}})
        ),
        Err(BridgeError::Unsupported)
    );
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

#[test]
fn commands_sharing_a_name_list_the_row_that_runs() {
    let mut bridge = initialized();
    let commands = bridge
        .inbound(
            json!({"type":"system","subtype":"commands_changed","commands":[
                {"name":"review","description":"Project review","argumentHint":""},
                {"name":"review","description":"Review a PR","argumentHint":"<pr>","builtin":true},
                {"name":"deploy","description":"Ship it","argumentHint":""},
                {"name":"deploy","description":"Other deploy","argumentHint":""},
                {"name":"cost","description":"Cost","argumentHint":"","builtin":true},
                {"name":"cost","description":"Later cost","argumentHint":"","builtin":true},
            ]}),
        )
        .unwrap();
    let listed = &updates(&commands)[0]["availableCommands"];
    assert_eq!(
        listed,
        &json!([
            {"name":"review","description":"Review a PR","input":{"hint":"<pr>"},"_meta":{"source":"builtin"}},
            {"name":"deploy","description":"Ship it","input":null},
            {"name":"cost","description":"Cost","input":null,"_meta":{"source":"builtin"}},
        ])
    );
}

#[test]
fn a_turn_is_undone_by_rewinding_to_its_message_and_a_task_is_stopped_by_id() {
    use crate::acp::{goal::NativeRpcReply, session_control::SessionControl};
    let mut bridge = initialized();
    const TURN: &str = "turn-6f9619ff-8b86-4011-b42d-00cf4fc964ff";
    let user = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{
            "sessionId":SESSION,"messageId":TURN,"prompt":[{"type":"text","text":"hi"}]}}),
        )
        .unwrap();
    assert_eq!(user["uuid"], "6f9619ff-8b86-4011-b42d-00cf4fc964ff");
    let undo = SessionControl::UndoTurn {
        turn_id: crate::acp::contract::SageTurnId::parse(TURN).unwrap(),
    };
    let rewind = bridge.session_control(&undo).unwrap();
    assert_eq!(rewind["request"]["subtype"], "rewind_files");
    assert_eq!(
        rewind["request"]["user_message_id"],
        "6f9619ff-8b86-4011-b42d-00cf4fc964ff"
    );
    assert!(bridge.controlling());
    assert_eq!(bridge.session_control(&undo), Err(BridgeError::Capacity));
    let done = bridge
        .inbound(success(
            &rewind,
            json!({"canRewind":true,"filesChanged":["a.rs","b.rs"]}),
        ))
        .unwrap();
    assert!(
        matches!(&done.native, Some(NativeRpcReply::Controlled(Some(text))) if text.as_str() == "Restored 2 files")
    );
    assert!(!bridge.controlling());
    let rewind = bridge.session_control(&undo).unwrap();
    let refused = bridge
        .inbound(success(
            &rewind,
            json!({"canRewind":false,"error":"No file checkpoint found for this message."}),
        ))
        .unwrap();
    assert!(
        matches!(&refused.native, Some(NativeRpcReply::ControlRefused(Some(text)))
        if text.as_str() == "No file checkpoint found for this message.")
    );
    let stop = bridge
        .session_control(&SessionControl::StopActivity {
            activity_id: crate::acp::intent::ExternalId::new("activityId", "task:task-7").unwrap(),
        })
        .unwrap();
    assert_eq!(
        stop["request"],
        json!({"subtype":"stop_task","task_id":"task-7"})
    );
    let failed = bridge
        .inbound(json!({"type":"control_response","response":{"subtype":"error","request_id":stop["request_id"],"error":"No task with that id"}}))
        .unwrap();
    assert!(failed.acp.is_empty());
    assert!(
        matches!(&failed.native, Some(NativeRpcReply::ControlRefused(Some(text))) if text.as_str() == "No task with that id")
    );
    // A turn id that names no SDK message cannot be undone.
    assert_eq!(
        bridge.session_control(&SessionControl::UndoTurn {
            turn_id: crate::acp::contract::SageTurnId::parse("turn-legacy").unwrap(),
        }),
        Err(BridgeError::Unsupported)
    );
}

/// The blocks `Mentions::blocks` sends for an `@` mention: the file's text
/// when embedded context is offered, otherwise a sized link.
#[test]
fn an_at_mention_reaches_claude_code_as_context_or_a_path_mention() {
    let (uri, path) = if cfg!(windows) {
        ("file:///C:/work/a.rs", "@C:\\work\\a.rs")
    } else {
        ("file:///work/a.rs", "@/work/a.rs")
    };
    for (block, expected) in [
        (
            json!({"type":"resource","resource":{"uri":uri,"text":"fn a"}}),
            format!("<context ref=\"{uri}\">\nfn a\n</context>"),
        ),
        (
            json!({"type":"resource_link","uri":uri,"name":"a.rs","size":4}),
            path.to_owned(),
        ),
    ] {
        let mut bridge = initialized();
        let user = bridge
            .outbound(
                &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"prompt":[
                    {"type":"text","text":"see @a.rs"},
                    block,
                ]}}),
            )
            .unwrap();
        assert_eq!(user["message"]["content"][1]["text"], expected);
    }
}

/// Claude Code reads an `@path` it receives, so a link to a credential file
/// Sage would not embed must not become one.
#[test]
fn a_mentioned_credential_file_is_named_but_not_handed_to_claude_code() {
    let uri = if cfg!(windows) {
        "file:///C:/work/.env"
    } else {
        "file:///work/.env"
    };
    let mut bridge = initialized();
    let user = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{"prompt":[
                {"type":"text","text":"see @.env"},
                {"type":"resource_link","uri":uri,"name":".env","size":9},
            ]}}),
        )
        .unwrap();
    let text = user["message"]["content"][1]["text"].as_str().unwrap();
    assert!(!text.contains('@'), "{text}");
    assert!(text.contains("Not attached"), "{text}");
}

fn lifecycle(uuid: &str, state: &str) -> Value {
    json!({"type":"command_lifecycle","command_uuid":uuid,"state":state,"uuid":"e1","session_id":SESSION})
}

fn ended(inbound: &Inbound) -> bool {
    inbound.acp.iter().any(|frame| frame["id"] == 3)
}

#[test]
fn a_message_sent_mid_turn_is_queued_and_the_turn_ends_after_it_runs() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (user, uuid) = bridge.steer("also check the tests").unwrap();
    assert_eq!(user["type"], "user");
    assert_eq!(user["uuid"], uuid.as_str());
    assert_eq!(
        user["message"]["content"],
        json!([{"type":"text","text":"also check the tests"}])
    );
    let queued = bridge.inbound(lifecycle(&uuid, "queued")).unwrap();
    assert_eq!(queued.steers, [(uuid.clone(), true)]);

    // The running reply ends before Claude Code takes the message up: the
    // turn goes on.
    let first = bridge.inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":1}})).unwrap();
    assert!(!ended(&first) && first.acp.is_empty());
    bridge.inbound(lifecycle(&uuid, "started")).unwrap();
    let chunk = bridge.inbound(json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Checked"}}})).unwrap();
    assert_eq!(updates(&chunk)[0]["content"]["text"], "Checked");
    let last = bridge.inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":2}})).unwrap();
    assert!(ended(&last));
    assert!(last.steers.is_empty());
    // A later lifecycle frame for it changes nothing.
    assert!(
        bridge
            .inbound(lifecycle(&uuid, "completed"))
            .unwrap()
            .acp
            .is_empty()
    );
}

#[test]
fn a_message_taken_up_at_a_tool_boundary_ends_with_the_one_result() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("stop after this").unwrap();
    bridge.inbound(lifecycle(&uuid, "queued")).unwrap();
    let started = bridge.inbound(lifecycle(&uuid, "started")).unwrap();
    assert_eq!(started.steers, [(uuid.clone(), true)]);
    bridge.inbound(lifecycle(&uuid, "completed")).unwrap();
    let result = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(ended(&result));
}

#[test]
fn a_held_turn_ends_when_its_waiting_message_is_dropped() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("later").unwrap();
    let first = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(!ended(&first));
    let dropped = bridge.inbound(lifecycle(&uuid, "cancelled")).unwrap();
    assert_eq!(dropped.steers, [(uuid, false)]);
    assert!(ended(&dropped));
}

#[test]
fn stopping_withdraws_waiting_messages_before_interrupting() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, first) = bridge.steer("one").unwrap();
    let (_, second) = bridge.steer("two").unwrap();
    let cancel = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    assert_eq!(
        cancel["request"],
        json!({"subtype":"cancel_async_message","message_uuid":first})
    );
    assert!(
        bridge.steer("three").is_err(),
        "nothing joins a stopping turn"
    );
    let next = bridge
        .inbound(success(&cancel, json!({"cancelled":true})))
        .unwrap();
    assert_eq!(next.steers, [(first, false)]);
    assert_eq!(
        next.replies[0]["request"],
        json!({"subtype":"cancel_async_message","message_uuid":second})
    );
    // A message already running is not withdrawn; the interrupt stops it.
    let next = bridge.inbound(json!({"type":"control_response","response":{"subtype":"error","request_id":next.replies[0]["request_id"],"error":"not queued"}})).unwrap();
    assert_eq!(next.steers, [(second, false)]);
    let interrupt = &next.replies[0];
    assert_eq!(interrupt["request"]["subtype"], "interrupt");
    bridge
        .inbound(success(interrupt, json!({"still_queued":[]})))
        .unwrap();
    let result = bridge
        .inbound(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}),
        )
        .unwrap();
    assert_eq!(
        result.acp.last().unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_message_needs_a_running_turn_and_an_unsent_one_is_forgotten() {
    let mut bridge = initialized();
    assert!(bridge.steer("idle").is_err());
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("never written").unwrap();
    bridge.undo(&Sent::Steer(uuid));
    let result = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(ended(&result));
}

#[test]
fn stopping_while_a_run_end_is_held_ends_the_turn_once_the_message_is_withdrawn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("later").unwrap();
    let first = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(!ended(&first));
    let cancel = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    let withdrawn = bridge
        .inbound(success(&cancel, json!({"cancelled":true})))
        .unwrap();
    assert_eq!(withdrawn.steers, [(uuid, false)]);
    assert!(
        withdrawn.replies.is_empty(),
        "Claude Code is idle; nothing to interrupt"
    );
    assert_eq!(
        withdrawn.acp.last().unwrap()["result"]["stopReason"],
        "cancelled"
    );
}

#[test]
fn a_late_withdrawal_reply_never_interrupts_the_next_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    bridge.steer("later").unwrap();
    let cancel = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    // The run ends before Claude Code answers the withdrawal: the turn
    // waits for the answer, then ends without reaching the next one.
    let end = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(!ended(&end));
    let answered = bridge
        .inbound(success(&cancel, json!({"cancelled":true})))
        .unwrap();
    assert!(answered.replies.is_empty());
    assert_eq!(
        answered.acp.last().unwrap()["result"]["stopReason"],
        "cancelled"
    );
    prompt(&mut bridge, 4);
    let next = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert_eq!(next.acp.last().unwrap()["result"]["stopReason"], "end_turn");
}

#[test]
fn a_message_taken_up_while_its_withdrawal_is_in_flight_is_interrupted_in_its_turn() {
    let mut bridge = initialized();
    prompt(&mut bridge, 3);
    let (_, uuid) = bridge.steer("later").unwrap();
    let cancel = bridge
        .outbound(
            &json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":SESSION}}),
        )
        .unwrap();
    // The run ends and Claude Code takes the message up before reading the
    // withdrawal.
    let first = bridge
        .inbound(json!({"type":"result","subtype":"success","is_error":false,"usage":{}}))
        .unwrap();
    assert!(!ended(&first));
    bridge.inbound(lifecycle(&uuid, "started")).unwrap();
    let reply = bridge
        .inbound(success(&cancel, json!({"cancelled":false})))
        .unwrap();
    assert_eq!(reply.replies[0]["request"]["subtype"], "interrupt");
    let end = bridge
        .inbound(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"usage":{}}),
        )
        .unwrap();
    assert_eq!(end.acp.last().unwrap()["result"]["stopReason"], "cancelled");
}

#[test]
fn a_fork_answers_once_claude_code_replies_and_never_mid_turn() {
    const REPLY: &str = "1c8e7d9f-4d2b-4a8f-8b62-3a7e9d2f5ca1";
    let config = std::env::temp_dir().join(format!("sage-sdk-fork-{}", uuid::Uuid::new_v4()));
    let project = config.join("projects").join("-w");
    std::fs::create_dir_all(&project).unwrap();
    let mut bridge = initialized();
    // The turn's reply reaches the transcript before Claude Code answers.
    let sdk = bridge.fork(Some(config.clone())).unwrap();
    assert_eq!(sdk["request"]["subtype"], "get_settings");
    assert!(bridge.controlling());
    std::fs::write(
        project.join(format!("{SESSION}.jsonl")),
        json!({"type":"assistant","uuid":REPLY,"isSidechain":false}).to_string(),
    )
    .unwrap();
    let reply = bridge.inbound(success(&sdk, json!({}))).unwrap();
    let Some(crate::acp::goal::NativeRpcReply::Forked(id)) = reply.native else {
        panic!("forked: {:?}", reply.native);
    };
    let fork = crate::acp::bindings::claude::sdk::fork::Fork::parse(id.as_str()).unwrap();
    assert_eq!((fork.source.as_str(), fork.at.as_str()), (SESSION, REPLY));
    // No profile, or Claude Code refusing, rejects the fork.
    let sdk = bridge.fork(None).unwrap();
    let reply = bridge.inbound(success(&sdk, json!({}))).unwrap();
    assert_eq!(
        reply.native,
        Some(crate::acp::goal::NativeRpcReply::Rejected)
    );
    let sdk = bridge.fork(Some(config.clone())).unwrap();
    let reply = bridge.inbound(json!({"type":"control_response","response":{"subtype":"error","request_id":sdk["request_id"],"error":"gone"}})).unwrap();
    assert_eq!(
        reply.native,
        Some(crate::acp::goal::NativeRpcReply::Rejected)
    );
    prompt(&mut bridge, 3);
    assert!(bridge.fork(None).is_err());
    let _ = std::fs::remove_dir_all(config);
}
