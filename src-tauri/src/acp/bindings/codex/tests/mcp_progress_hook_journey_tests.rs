use super::*;
use sage_acp_protocol::{
    intent::{
        ToolCategory,
        session_event::{SessionActivityState, SessionScopedEvent},
    },
    lifecycle::{ItemLifecycle, ToolState},
};

#[derive(Debug, PartialEq, Eq)]
enum Shown {
    Tool(String, ItemLifecycle, Option<ToolState>, Option<String>),
    Hook(String, String, SessionActivityState, Option<String>),
}

fn replay(case: &str) -> Vec<Shown> {
    let requests = [
        json!({"id":900,"method":"initialize"}),
        json!({"id":1,"method":"thread/start"}),
        json!({"id":3,"method":"thread/goal/get"}),
        json!({"id":2,"method":"turn/start"}),
        json!({"id":4,"method":"thread/unsubscribe"}),
    ]
    .iter()
    .map(|request| format!("{request}\n"))
    .collect::<String>();
    let frames = crate::acp::fixture::replay_native(case, &requests);
    let mut adapter = adapter();
    negotiate(&mut adapter);
    adapter
        .start(1, &adapter.project_root(r"C:\repo".into()).unwrap(), None)
        .unwrap();
    let thread = frames.iter().find(|frame| frame["id"] == 1).unwrap();
    adapter
        .ingest(&serde_json::to_vec(thread).unwrap(), &Mapper)
        .unwrap();
    arm_turn(&mut adapter, 2, "client-journey");
    let turn = frames.iter().position(|frame| frame["id"] == 2).unwrap();
    let mut shown = Vec::new();
    for frame in &frames[turn..] {
        let disposition = adapter
            .ingest(&serde_json::to_vec(frame).unwrap(), &Mapper)
            .unwrap_or_else(|error| panic!("{case} refused {frame}: {error:?}"));
        let IngressDisposition::Events(events) = disposition else {
            panic!("{case} dropped {frame}: {disposition:?}")
        };
        shown.extend(events.into_iter().filter_map(projected));
        if frame["method"] == "turn/completed" {
            return shown;
        }
    }
    panic!("{case} never completed its turn")
}

fn projected(event: IncomingEvent) -> Option<Shown> {
    match event {
        IncomingEvent::Item {
            id,
            lifecycle,
            tool_state,
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Mcp,
                    execution,
                    ..
                },
            ..
        } => Some(Shown::Tool(
            id.external_id.as_str().to_owned(),
            lifecycle,
            tool_state,
            execution.progress.map(|line| line.as_str().to_owned()),
        )),
        IncomingEvent::Session {
            event: SessionScopedEvent::Activity(activity),
            ..
        } => {
            assert_eq!(activity.kind.as_str(), "hook");
            Some(Shown::Hook(
                activity.id.as_str().to_owned(),
                activity.label.as_str().to_owned(),
                activity.state,
                activity.progress.map(|line| line.as_str().to_owned()),
            ))
        }
        _ => None,
    }
}

fn tool(
    lifecycle: ItemLifecycle,
    state: ToolState,
    progress: Option<&str>,
) -> (ItemLifecycle, Option<ToolState>, Option<String>) {
    (lifecycle, Some(state), progress.map(str::to_owned))
}

#[test]
fn mcp_progress_journey_shows_each_report_on_the_running_call_then_completes() {
    let shown = replay("codex-mcp-progress");
    let rows: Vec<_> = shown
        .into_iter()
        .map(|entry| match entry {
            Shown::Tool(id, lifecycle, state, progress) => {
                assert_eq!(id, "fixture-mcp-progress");
                (lifecycle, state, progress)
            }
            Shown::Hook(..) => panic!("the MCP journey runs no hook"),
        })
        .collect();
    assert_eq!(
        rows,
        [
            tool(ItemLifecycle::Started, ToolState::InProgress, None),
            tool(
                ItemLifecycle::Streaming,
                ToolState::InProgress,
                Some("Indexing synthetic docs (1 of 2)")
            ),
            tool(
                ItemLifecycle::Streaming,
                ToolState::InProgress,
                Some("Indexing synthetic docs (2 of 2)")
            ),
            tool(ItemLifecycle::Completed, ToolState::Completed, None),
        ]
    );
}

#[test]
fn hook_journey_settles_a_passing_and_a_failing_hook_around_the_tool_call() {
    let shown = replay("codex-hook-run");
    let [
        Shown::Hook(pre, pre_label, SessionActivityState::Running, pre_line),
        Shown::Hook(pre_done, _, SessionActivityState::Completed, None),
        Shown::Tool(_, ItemLifecycle::Started, _, None),
        Shown::Tool(_, ItemLifecycle::Completed, Some(ToolState::Completed), None),
        Shown::Hook(post, post_label, SessionActivityState::Running, post_line),
        Shown::Hook(post_done, _, SessionActivityState::Failed, failure),
    ] = shown.as_slice()
    else {
        panic!("unexpected hook journey projection: {shown:?}")
    };
    assert_eq!(pre_label, "PreToolUse hook #1 (project)");
    assert_eq!(pre_line.as_deref(), Some("Checking synthetic policy"));
    assert_eq!(pre_done, pre);
    assert_eq!(post_label, "PostToolUse hook #2 (project)");
    assert_eq!(post_line.as_deref(), Some("Linting synthetic output"));
    assert_eq!(
        failure.as_deref(),
        Some("Synthetic lint hook exited with status 1")
    );
    assert_eq!(post_done, post);
    assert_ne!(pre, post, "each handler keeps its own row");
}
