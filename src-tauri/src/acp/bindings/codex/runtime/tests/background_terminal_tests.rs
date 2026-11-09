//! `thread/backgroundTerminals/*` and terminal input on the command row.

use super::*;
use crate::acp::{
    background_terminals::{BackgroundTerminal, TerminalAction},
    goal::NativeRpcReply,
    intent::SafeResourceLocation,
};

fn send(runtime: &mut CodexRuntime, action: TerminalAction) -> (u64, Value) {
    let egress = runtime
        .background_terminals(&action)
        .expect("terminal request");
    let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    (request_id(&egress), wire)
}

fn answer(runtime: &mut CodexRuntime, id: u64, body: Value) -> Option<NativeRpcReply> {
    let mut reply = json!({ "id": id });
    reply
        .as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    let batch = runtime.ingest(&frame(reply), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    runtime.take_native_rpc_reply()
}

#[test]
fn a_listing_maps_each_terminal_to_a_handle_a_safe_command_and_a_mapped_cwd() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    assert!(runtime.capabilities().unwrap().native_background_terminals);

    let (id, wire) = send(&mut runtime, TerminalAction::List);
    assert_eq!(wire["method"], "thread/backgroundTerminals/list");
    assert_eq!(wire["params"], json!({"threadId": "thread-1", "limit": 32}));
    assert!(runtime.native_rpc_pending());
    assert!(runtime.background_terminals(&TerminalAction::List).is_err());

    let reply = answer(
        &mut runtime,
        id,
        json!({"result": {"data": [
            {"processId": "7", "itemId": "call-1", "command": "npm run\tdev",
             "cwd": "C:\\repo\\src\\lib.rs", "osPid": 41, "cpuPercent": 1.5, "rssKb": 900},
            {"processId": "8", "itemId": "call-2", "command": "curl -H 'Bearer opaque-credential'",
             "cwd": "D:\\elsewhere"}
        ], "nextCursor": null}}),
    );
    let Some(NativeRpcReply::BackgroundTerminals(terminals)) = reply else {
        panic!("listing did not answer: {reply:?}");
    };
    assert!(!runtime.native_rpc_pending());
    assert_eq!(
        terminals[0],
        BackgroundTerminal {
            handle: "7".into(),
            command: "npm run dev".into(),
            cwd: Some(SafeResourceLocation::Resource {
                resource: SafeResourceRef::new(
                    SageRootId::parse("root-runtime").unwrap(),
                    "src/lib.rs"
                )
                .unwrap(),
            }),
        }
    );
    assert!(!terminals[1].command.contains("opaque-credential"));
    assert!(matches!(
        terminals[1].cwd,
        Some(SafeResourceLocation::Unavailable { .. })
    ));
    assert!(!format!("{terminals:?}").contains("elsewhere"));
}

#[test]
fn a_listing_is_capped_and_an_off_schema_or_refused_answer_is_rejected() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let many: Vec<Value> = (0..40)
        .map(|n| json!({"processId": n.to_string(), "itemId": "i", "command": "sleep 9", "cwd": "C:\\repo"}))
        .collect();
    let (id, _) = send(&mut runtime, TerminalAction::List);
    let Some(NativeRpcReply::BackgroundTerminals(terminals)) =
        answer(&mut runtime, id, json!({"result": {"data": many}}))
    else {
        panic!("capped listing");
    };
    assert_eq!(terminals.len(), 32);

    for body in [
        json!({"result": {"data": "none"}}),
        json!({"result": {"data": [{"processId": "", "command": "x", "cwd": "C:\\repo"}]}}),
        json!({"error": {"code": -32600, "message": "unknown thread"}}),
    ] {
        let (id, _) = send(&mut runtime, TerminalAction::List);
        assert_eq!(
            answer(&mut runtime, id, body),
            Some(NativeRpcReply::Rejected)
        );
    }
}

#[test]
fn stop_and_stop_all_use_the_pinned_methods() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, wire) = send(&mut runtime, TerminalAction::Stop("7".into()));
    assert_eq!(wire["method"], "thread/backgroundTerminals/terminate");
    assert_eq!(
        wire["params"],
        json!({"threadId": "thread-1", "processId": "7"})
    );
    // A terminal that already exited is not an error.
    assert_eq!(
        answer(&mut runtime, id, json!({"result": {"terminated": false}})),
        Some(NativeRpcReply::TerminalsStopped)
    );
    let (id, wire) = send(&mut runtime, TerminalAction::StopAll);
    assert_eq!(wire["method"], "thread/backgroundTerminals/clean");
    assert_eq!(wire["params"], json!({"threadId": "thread-1"}));
    assert_eq!(
        answer(&mut runtime, id, json!({"result": {}})),
        Some(NativeRpcReply::TerminalsStopped)
    );
}

#[test]
fn no_terminal_request_is_sent_before_a_thread_is_bound() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    assert!(runtime.background_terminals(&TerminalAction::List).is_err());
}

#[test]
fn terminal_input_joins_the_running_command_output() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"item/started","params":{
                "threadId":"thread-1","turnId":"turn-1","item":{
                    "id":"command-1","type":"commandExecution","status":"inProgress",
                    "command":"python -i","cwd":"C:\\repo","commandActions":[],
                    "processId":"7","source":"unifiedExecStartup"
                }
            }})),
            &Mapper,
        )
        .expect("command start");
    let interaction = |item: &str, stdin: &str| {
        frame(
            json!({"method":"item/commandExecution/terminalInteraction","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":item,"processId":"7","stdin":stdin
            }}),
        )
    };
    let typed = runtime
        .ingest(&interaction("command-1", "print(42)\n"), &Mapper)
        .expect("terminal input");
    assert!(matches!(
        typed.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { output_summary: Some(output), .. },
            ..
        }] if output.as_str().contains("print(42)")
    ));
    // Input to a command no open row holds is not projected.
    let earlier = runtime
        .ingest(&interaction("command-from-earlier-turn", "q\n"), &Mapper)
        .expect("earlier command input");
    assert!(matches!(earlier.events(), [AdapterEvent::IgnoredLate]));
    let unnamed = frame(
        json!({"method":"item/commandExecution/terminalInteraction","params":{
            "threadId":"thread-1","turnId":"turn-1","itemId":"command-1","stdin":"x"
        }}),
    );
    assert!(runtime.ingest(&unnamed, &Mapper).is_err());
}
