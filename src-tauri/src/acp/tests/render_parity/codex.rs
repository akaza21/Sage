use serde_json::{Value, json};

use super::{FILE, Fact, Mapper, ROOT, Scenario, frame};
use crate::acp::{
    bindings::{
        adapter::{AdapterCommand, AdapterEgress, AdapterEvent, EgressResolution, HarnessAdapter},
        codex::{CodexRuntime, Platform},
    },
    transcript_quote::Prompt,
};
use sage_acp_protocol::contract::RequiredText;

const DIFF: &str = "@@ -1,3 +1,3 @@\n pub fn answer() -> u32 {\n-    41\n+    42\n }\n";

fn params(mut params: Value) -> Value {
    params["threadId"] = json!("thread-1");
    params["turnId"] = json!("turn-1");
    params
}

fn notify(method: &str, value: Value) -> Value {
    json!({"method":method,"params":params(value)})
}

fn item(method: &str, item: Value) -> Value {
    notify(method, json!({ "item": item }))
}

fn delta(method: &str, id: &str, delta: &str) -> Value {
    notify(method, json!({"itemId":id,"delta":delta,"summaryIndex":0}))
}

fn read(status: &str, output: Option<&str>) -> Value {
    json!({
        "type":"commandExecution","id":"tool-1","command":"cat src/lib.rs","cwd":ROOT,
        "status":status,"aggregatedOutput":output,"exitCode":output.map(|_| if status == "failed" {1} else {0}),
        "commandActions":[{"type":"read","command":"cat src/lib.rs","name":"lib.rs","path":FILE}]
    })
}

fn command(command: &str, status: &str, output: Option<&str>, exit: Option<i32>) -> Value {
    json!({
        "type":"commandExecution","id":"tool-1","command":command,"cwd":r"C:\repo\src","status":status,
        "aggregatedOutput":output,"exitCode":exit,"commandActions":[{"type":"unknown","command":command}]
    })
}

fn frames(scenario: Scenario) -> (Vec<Value>, &'static str) {
    let frames = match scenario {
        Scenario::ThinkingThenLinkedText => vec![
            item(
                "item/started",
                json!({"type":"reasoning","id":"thought-1","summary":[],"content":[]}),
            ),
            delta(
                "item/reasoning/summaryTextDelta",
                "thought-1",
                "Checking the guide.",
            ),
            item(
                "item/completed",
                json!({"type":"reasoning","id":"thought-1","summary":["Checking the guide."],"content":[]}),
            ),
            item(
                "item/started",
                json!({"type":"agentMessage","id":"message-1","text":""}),
            ),
            delta(
                "item/agentMessage/delta",
                "message-1",
                "See [the guide](https://example.com/guide).",
            ),
            item(
                "item/completed",
                json!({"type":"agentMessage","id":"message-1","text":"See [the guide](https://example.com/guide)."}),
            ),
        ],
        Scenario::ToolCompletes => vec![
            item("item/started", read("inProgress", None)),
            item("item/completed", read("completed", Some(super::OLD_TEXT))),
        ],
        Scenario::ToolFailsMidTurn => vec![
            item("item/started", read("inProgress", None)),
            item("item/completed", read("failed", Some("Permission denied"))),
            item(
                "item/completed",
                json!({"type":"agentMessage","id":"message-1","text":"The read failed."}),
            ),
        ],
        Scenario::CommandWithOutput => vec![
            item(
                "item/started",
                command("echo hello", "inProgress", None, None),
            ),
            notify(
                "item/commandExecution/outputDelta",
                json!({"itemId":"tool-1","delta":"hello"}),
            ),
            item(
                "item/completed",
                command("echo hello", "completed", Some("hello"), Some(0)),
            ),
        ],
        Scenario::FileEditWithDiff => {
            let change = |status: &str| {
                json!({"type":"fileChange","id":"tool-1","status":status,
                    "changes":[{"path":FILE,"kind":{"type":"update","move_path":null},"diff":DIFF}]})
            };
            vec![
                item("item/started", change("inProgress")),
                item("item/completed", change("completed")),
            ]
        }
        Scenario::PlanUpdate => vec![
            notify(
                "turn/plan/updated",
                json!({"explanation":null,"plan":[
                    {"step":"Inspect","status":"inProgress"},{"step":"Edit","status":"pending"}
                ]}),
            ),
            notify(
                "turn/plan/updated",
                json!({"explanation":null,"plan":[
                    {"step":"Inspect","status":"completed"},{"step":"Edit","status":"inProgress"},
                    {"step":"Test","status":"pending"}
                ]}),
            ),
        ],
        Scenario::CancelledWithRunningTool => vec![item("item/started", read("inProgress", None))],
        Scenario::SteeredMessage => vec![item(
            "item/completed",
            json!({"type":"agentMessage","id":"message-1","text":"Starting on lib.rs."}),
        )],
    };
    let status = match scenario {
        Scenario::CancelledWithRunningTool => "interrupted",
        _ => "completed",
    };
    (frames, status)
}

fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

fn answer(runtime: &mut CodexRuntime, egress: AdapterEgress, result: Value) -> Vec<AdapterEvent> {
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    let batch = runtime
        .ingest(&frame(&json!({"id":id,"result":result})), &Mapper::repo())
        .unwrap();
    for egress in batch.egress() {
        runtime
            .resolve_egress(egress.token().clone(), EgressResolution::Queued)
            .unwrap();
    }
    batch.events().to_vec()
}

pub(super) fn run(scenario: Scenario) -> Option<Vec<Fact>> {
    let mut runtime = CodexRuntime::fixture(Platform::WindowsX86_64);
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    answer(
        &mut runtime,
        initialize,
        json!({
            "userAgent":"sage/0.167.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
            "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.167.0-486c7c8d",
            "platformFamily":"windows","platformOs":"windows"
        }),
    );
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: ROOT.into(),
            model: None,
        })
        .unwrap();
    answer(&mut runtime, session, json!({"thread":{"id":"thread-1"}}));
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Render parity").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    answer(&mut runtime, prompt, json!({"turn":{"id":"turn-1"}}));
    let (frames, status) = frames(scenario);
    let mut facts = Vec::new();
    for value in std::iter::once(json!({
        "method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1"}}
    }))
    .chain(frames)
    {
        facts.extend(ingest(&mut runtime, value));
    }
    if scenario == Scenario::SteeredMessage {
        // The user steers: Codex accepts `turn/steer`, then echoes the
        // message as its own `userMessage` item before answering it.
        let steer = runtime
            .command(AdapterCommand::Steer {
                prompt: Prompt::new("prompt", super::STEER).unwrap(),
                client_message_id: RequiredText::new("client", super::STEER_CLIENT_ID).unwrap(),
            })
            .unwrap();
        let request: Value = serde_json::from_slice(steer.frame().payload()).unwrap();
        assert_eq!(request["method"], "turn/steer");
        facts.extend(
            answer(&mut runtime, steer, json!({"turnId":"turn-1"}))
                .into_iter()
                .filter_map(super::adapter_fact),
        );
        let echo = json!({"type":"userMessage","id":"echo-1",
            "content":[{"type":"text","text":super::STEER,"text_elements":[]}]});
        for value in [
            item("item/started", echo.clone()),
            item("item/completed", echo),
            item(
                "item/completed",
                json!({"type":"agentMessage","id":"message-2","text":"Switching to main.rs."}),
            ),
        ] {
            facts.extend(ingest(&mut runtime, value));
        }
    }
    if scenario == Scenario::CancelledWithRunningTool {
        // A user stop: Sage records the cancel intent and writes
        // `turn/interrupt`; the app server acknowledges it and then ends the
        // turn as interrupted.
        let interrupt = runtime.command(AdapterCommand::Cancel).unwrap();
        let request: Value = serde_json::from_slice(interrupt.frame().payload()).unwrap();
        assert_eq!(request["method"], "turn/interrupt");
        facts.push(Fact::UserCancel);
        facts.extend(
            answer(&mut runtime, interrupt, json!({}))
                .into_iter()
                .filter_map(super::adapter_fact),
        );
    }
    facts.extend(ingest(
        &mut runtime,
        json!({
            "method":"turn/completed",
            "params":{"threadId":"thread-1","turn":{"id":"turn-1","status":status,"items":[]}}
        }),
    ));
    Some(facts)
}

fn ingest(runtime: &mut CodexRuntime, value: Value) -> Vec<Fact> {
    let batch = runtime
        .ingest(&frame(&value), &Mapper::repo())
        .unwrap_or_else(|error| panic!("codex refused {value}: {error:?}"));
    let events = batch.events().iter().cloned();
    events.filter_map(super::adapter_fact).collect()
}
