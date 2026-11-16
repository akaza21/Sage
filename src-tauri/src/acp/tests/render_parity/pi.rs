use serde_json::{Value, json};

use super::{FILE, Fact, Mapper, NEW_TEXT, OLD_TEXT, ROOT, Scenario, frame};
use crate::acp::{
    bindings::{
        adapter::{AdapterBatch, AdapterCommand, AdapterEgress, EgressResolution, HarnessAdapter},
        pi::PiRuntime,
    },
    transcript_quote::Prompt,
};
use sage_acp_protocol::access_mode::SessionAccessMode;
use sage_acp_protocol::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    permissions::PermissionDecision,
};

struct Turn {
    runtime: PiRuntime,
    mapper: Mapper,
    facts: Vec<Fact>,
}

impl Turn {
    fn new(mapper: Mapper, workspace: Option<&std::path::Path>) -> Self {
        let mut runtime = PiRuntime::fixture(
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("session-pi-parity").unwrap(),
        );
        if let Some(root) = workspace {
            runtime
                .enable_bridge(sage_workspace::Workspace::open("project", "root-1", root).unwrap());
        }
        let mut turn = Self {
            runtime,
            mapper,
            facts: Vec::new(),
        };
        let state = turn.runtime.command(AdapterCommand::Initialize).unwrap();
        let models = turn.reply(
            state,
            json!({"model":{"provider":"openai","id":"gpt","name":"GPT"}}),
        );
        turn.reply(
            models.expect("models request"),
            json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]}),
        );
        let session = turn
            .runtime
            .command(AdapterCommand::NewSession {
                native_root: ROOT.into(),
                model: None,
            })
            .unwrap();
        turn.reply(session, json!({"sessionId":"parity-session"}));
        let prompt = turn
            .runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Render parity").unwrap(),
                client_message_id: RequiredText::new("client", "client-1").unwrap(),
                access_mode: SessionAccessMode::WorkspaceWrite,
            })
            .unwrap();
        turn.reply(prompt, Value::Null);
        turn.facts.clear();
        turn.send(json!({"type":"agent_start"}));
        turn
    }

    fn deliver(&mut self, egress: AdapterEgress) -> Value {
        let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        let token = egress.into_token();
        for resolution in [EgressResolution::Queued, EgressResolution::Written] {
            self.runtime
                .resolve_egress(token.clone(), resolution)
                .unwrap();
        }
        request
    }

    fn reply(&mut self, egress: AdapterEgress, data: Value) -> Option<AdapterEgress> {
        let request = self.deliver(egress);
        let batch = self
            .runtime
            .ingest(
                &frame(&json!({"type":"response","id":request["id"],
                    "command":request["type"],"success":true,"data":data})),
                &self.mapper,
            )
            .unwrap();
        self.record(&batch);
        let mut egress = batch.into_parts().1.into_iter();
        egress.next()
    }

    fn send(&mut self, value: Value) {
        let batch = self
            .runtime
            .ingest(&frame(&value), &self.mapper)
            .unwrap_or_else(|error| panic!("pi refused {value}: {error:?}"));
        self.record(&batch);
        for egress in batch.into_parts().1 {
            self.deliver(egress);
        }
    }

    fn settle(mut self, stop: &str) -> Vec<Fact> {
        self.send(
            json!({"type":"agent_end","messages":[{"role":"assistant","content":[],
            "stopReason":stop,"usage":{"input":2,"output":1}}],"willRetry":false}),
        );
        self.send(json!({"type":"agent_settled"}));
        self.facts
    }

    fn record(&mut self, batch: &AdapterBatch) {
        let facts = batch.events().iter().cloned();
        self.facts.extend(facts.filter_map(super::adapter_fact));
    }
}

fn message(turn: &mut Turn, events: &[(&str, &str)]) {
    turn.send(json!({"type":"message_start","message":{"role":"assistant"}}));
    for (kind, delta) in events {
        turn.send(json!({"type":"message_update","assistantMessageEvent":{"type":kind,"contentIndex":0,"delta":delta,"content":delta}}));
    }
    turn.send(json!({"type":"message_end","message":{"role":"assistant"}}));
}

fn tool(turn: &mut Turn, name: &str, args: Value, result: Option<(&str, bool)>) {
    turn.send(
        json!({"type":"tool_execution_start","toolCallId":"tool-1","toolName":name,"args":args}),
    );
    if let Some((text, failed)) = result {
        turn.send(
            json!({"type":"tool_execution_end","toolCallId":"tool-1","toolName":name,
            "result":{"content":[{"type":"text","text":text}]},"isError":failed}),
        );
    }
}

pub(super) fn run(scenario: Scenario) -> Option<Vec<Fact>> {
    let read = json!({ "path": FILE });
    Some(match scenario {
        Scenario::ThinkingThenLinkedText => {
            let mut turn = Turn::new(Mapper::repo(), None);
            message(
                &mut turn,
                &[
                    ("thinking_delta", "Checking the guide."),
                    ("thinking_end", "Checking the guide."),
                    ("text_delta", "See [the guide](https://example.com/guide)."),
                ],
            );
            turn.settle("stop")
        }
        Scenario::ToolCompletes => {
            let mut turn = Turn::new(Mapper::repo(), None);
            tool(&mut turn, "read", read, Some((OLD_TEXT, false)));
            turn.settle("stop")
        }
        Scenario::ToolFailsMidTurn => {
            let mut turn = Turn::new(Mapper::repo(), None);
            tool(&mut turn, "read", read, Some(("Permission denied", true)));
            message(&mut turn, &[("text_delta", "The read failed.")]);
            turn.settle("stop")
        }
        Scenario::FileEditWithDiff => return Some(edit()),
        Scenario::CancelledWithRunningTool => {
            let mut turn = Turn::new(Mapper::repo(), None);
            tool(&mut turn, "read", read, None);
            let abort = turn.runtime.command(AdapterCommand::Cancel).unwrap();
            // A user stop: Sage records the cancel intent, writes Pi's
            // `abort`, Pi accepts it and ends the run as aborted.
            turn.facts.push(Fact::UserCancel);
            assert!(turn.reply(abort, Value::Null).is_none());
            turn.settle("aborted")
        }
        Scenario::SteeredMessage => {
            let mut turn = Turn::new(Mapper::repo(), None);
            message(&mut turn, &[("text_delta", "Starting on lib.rs.")]);
            // The user steers: Pi accepts `steer`, then echoes the message as
            // its own user message before answering it.
            let steer = turn
                .runtime
                .command(AdapterCommand::Steer {
                    prompt: Prompt::new("prompt", super::STEER).unwrap(),
                    client_message_id: RequiredText::new("client", super::STEER_CLIENT_ID).unwrap(),
                })
                .unwrap();
            assert!(turn.reply(steer, json!({"disposition":"queued"})).is_none());
            let user = json!({"role":"user","content":[{"type":"text","text":super::STEER}],"timestamp":0});
            turn.send(json!({"type":"message_start","message":user}));
            turn.send(json!({"type":"message_end","message":user}));
            message(&mut turn, &[("text_delta", "Switching to main.rs.")]);
            turn.settle("stop")
        }
        // Sage's Pi bridge refuses every shell command, and Pi has no plan.
        Scenario::CommandWithOutput | Scenario::PlanUpdate => return None,
    })
}

fn edit() -> Vec<Fact> {
    let root = std::env::temp_dir().join(format!("sage-render-parity-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), OLD_TEXT).unwrap();
    // Pi runs in the project root Sage launches it in, which is always the
    // canonical path (`ProjectRootAuthority`), so the paths Pi reports are
    // under it. A temp dir is not canonical everywhere: on Windows it can be
    // an 8.3 short name or lack the verbatim prefix, and the bridge would
    // refuse the edit as outside the workspace.
    let root = root.canonicalize().unwrap();
    let file = root.join("src/lib.rs").to_str().unwrap().to_owned();
    let mut turn = Turn::new(Mapper(root.to_str().unwrap().to_owned()), Some(&root));
    let args = json!({"path":file,"oldText":"41","newText":"42"});
    tool(&mut turn, "edit", args.clone(), None);
    let dialog = |id: &str, title: &str, body: String| json!({"type":"extension_ui_request","method":"input","id":id,"title":title,"placeholder":body});
    turn.send(dialog(
        "prepare-1",
        "sage.pi.prepare.v1",
        json!({"version":1,"toolCallId":"tool-1","tool":"edit","args":args}).to_string(),
    ));
    let allow = turn
        .runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("prepare-1").unwrap(),
            decision: PermissionDecision::AllowOnce,
        })
        .unwrap();
    turn.deliver(allow);
    turn.send(dialog("commit-1", "sage.pi.commit.v1", "tool-1".into()));
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
        NEW_TEXT
    );
    turn.send(
        json!({"type":"tool_execution_end","toolCallId":"tool-1","toolName":"edit",
        "result":{"content":[{"type":"text","text":"Edited src/lib.rs"}]},"isError":false}),
    );
    let facts = turn.settle("stop");
    std::fs::remove_dir_all(root).unwrap();
    facts
}
