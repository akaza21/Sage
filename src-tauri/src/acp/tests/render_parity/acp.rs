//! Standard ACP wire for each scenario, through the shared translator with
//! Claude's profile (the current Claude binding) or a plain ACP profile.

use serde_json::{Value, json};

use super::{FILE, Fact, Mapper, NEW_TEXT, OLD_TEXT, Scenario};
use crate::acp::bindings::standard_acp::{
    profile_for,
    translator::{AdapterEvent, StandardAcpTranslator},
};

const SESSION: &str = "acp-session-1";

pub(super) fn claude(scenario: Scenario) -> Option<Vec<Fact>> {
    Some(run("claude-code", scenario))
}

/// No vendor updates and no command field: what any ACP agent gets.
pub(super) fn generic(scenario: Scenario) -> Option<Vec<Fact>> {
    Some(run("gemini-cli", scenario))
}

fn update(update: Value) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":SESSION,"update":update}})
}

fn text(kind: &str, id: &str, text: &str) -> Value {
    update(json!({"sessionUpdate":kind,"messageId":id,"content":{"type":"text","text":text}}))
}

fn read_call(status: &str) -> Value {
    update(json!({
        "sessionUpdate":"tool_call","toolCallId":"tool-1","title":"Read src/lib.rs",
        "kind":"read","status":status,"locations":[{"path":FILE}],"rawInput":{"file_path":FILE}
    }))
}

fn settle(status: &str, output: &str) -> Value {
    update(json!({
        "sessionUpdate":"tool_call_update","toolCallId":"tool-1","status":status,
        "content":[{"type":"content","content":{"type":"text","text":output}}]
    }))
}

fn frames(scenario: Scenario) -> (Vec<Value>, &'static str) {
    let frames = match scenario {
        Scenario::ThinkingThenLinkedText => vec![
            text("agent_thought_chunk", "thought-1", "Checking the guide."),
            text(
                "agent_message_chunk",
                "message-1",
                "See [the guide](https://example.com/guide).",
            ),
        ],
        Scenario::ToolCompletes => vec![read_call("pending"), settle("completed", OLD_TEXT)],
        Scenario::ToolFailsMidTurn => vec![
            read_call("pending"),
            settle("failed", "Permission denied"),
            text("agent_message_chunk", "message-1", "The read failed."),
        ],
        Scenario::CommandWithOutput => vec![
            update(json!({
                "sessionUpdate":"tool_call","toolCallId":"tool-1","title":"echo hello",
                "kind":"execute","status":"in_progress","rawInput":{"command":"echo hello"}
            })),
            // The exit status in the ACP `TerminalExitStatus` spelling.
            update(json!({
                "sessionUpdate":"tool_call_update","toolCallId":"tool-1","status":"completed",
                "content":[{"type":"content","content":{"type":"text","text":"hello"}}],
                "rawOutput":{"exitCode":0}
            })),
        ],
        Scenario::FileEditWithDiff => vec![
            update(json!({
                "sessionUpdate":"tool_call","toolCallId":"tool-1","title":"Edit src/lib.rs",
                "kind":"edit","status":"pending","locations":[{"path":FILE}],
                "rawInput":{"file_path":FILE,"old_string":"41","new_string":"42"}
            })),
            update(json!({
                "sessionUpdate":"tool_call_update","toolCallId":"tool-1","status":"completed",
                "content":[{"type":"diff","path":FILE,"oldText":OLD_TEXT,"newText":NEW_TEXT}]
            })),
        ],
        Scenario::PlanUpdate => vec![
            update(json!({"sessionUpdate":"plan","entries":[
                {"content":"Inspect","priority":"high","status":"in_progress"},
                {"content":"Edit","priority":"high","status":"pending"}
            ]})),
            update(json!({"sessionUpdate":"plan","entries":[
                {"content":"Inspect","priority":"high","status":"completed"},
                {"content":"Edit","priority":"high","status":"in_progress"},
                {"content":"Test","priority":"medium","status":"pending"}
            ]})),
        ],
        Scenario::CancelledWithRunningTool => vec![read_call("in_progress")],
    };
    let stop = match scenario {
        Scenario::CancelledWithRunningTool => "cancelled",
        _ => "end_turn",
    };
    (frames, stop)
}

fn run(profile: &str, scenario: Scenario) -> Vec<Fact> {
    let mut translator =
        StandardAcpTranslator::new(profile_for(profile).expect("profile"), SESSION).unwrap();
    translator.begin_prompt("3").unwrap();
    let (frames, stop) = frames(scenario);
    let mut facts = Vec::new();
    for frame in frames {
        facts.extend(
            translator
                .translate_notification_value(frame.clone(), &Mapper::repo())
                .unwrap_or_else(|error| panic!("{profile} refused {frame}: {error:?}"))
                .into_iter()
                .filter_map(fact),
        );
    }
    if scenario == Scenario::CancelledWithRunningTool {
        // A user stop: Sage records the cancel intent and the runtime writes
        // the `session/cancel` notification, which has no reply and never
        // reaches the translator. The agent answers by settling the pending
        // `session/prompt` with stopReason `cancelled`.
        facts.push(Fact::UserCancel);
    }
    facts.extend(
        translator
            .translate_prompt_result_value(
                json!({"jsonrpc":"2.0","id":3,"result":{"stopReason":stop}}),
            )
            .expect("prompt result")
            .into_iter()
            .filter_map(fact),
    );
    facts
}

fn fact(event: AdapterEvent) -> Option<Fact> {
    Some(match event {
        AdapterEvent::Item {
            external_key,
            lifecycle,
            tool_state,
            payload,
        } => Fact::Item {
            key: external_key.as_str().to_owned(),
            lifecycle,
            tool_state,
            payload,
        },
        AdapterEvent::FilePair {
            external_key,
            old_text,
            new_text,
        } => Fact::Pair {
            key: external_key.as_str().to_owned(),
            old_text,
            new_text,
        },
        AdapterEvent::TurnTerminal(terminal) => Fact::Turn(terminal.turn_state()),
        _ => return None,
    })
}
