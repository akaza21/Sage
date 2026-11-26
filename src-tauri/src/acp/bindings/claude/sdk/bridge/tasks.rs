//! Claude Code reports each longer command as a task, foreground ones too,
//! and the command's own tool row already shows those. Only a task that runs
//! in the background becomes a session activity.

use std::collections::VecDeque;

use serde_json::Value;

use super::notices::{TASK_PREFIX, activity, clip};
use super::{Inbound, SdkBridge};
use sage_acp_protocol::intent::session_event::{
    SESSION_ACTIVITY_LABEL_BYTES, SessionActivityState,
};

/// The protocol's activity kind for a harness process that outlives its turn.
const BACKGROUND_TASK: &str = "backgroundTask";
/// Tasks remembered, oldest dropped first: only recent ones still report.
const TRACKED_TASKS: usize = 64;
const ID_BYTES: usize = 256;

#[derive(Debug, Default)]
pub(super) struct Tasks {
    /// By task id: whether it runs in the background, and its description.
    seen: VecDeque<(String, bool, Option<String>)>,
    /// The latest background task to end since Claude Code last ran.
    ended: Option<String>,
    /// Runs Claude Code started on its own, numbering their rows.
    own_runs: u64,
}

impl Tasks {
    /// The task's description when the event belongs to a background task.
    /// `task_started` says whether it starts in the background (a CLI that
    /// does not say is taken to), and `task_updated` can move it there.
    fn background(&mut self, id: &str, sdk: &Value) -> Option<Option<&str>> {
        let text = |key: &str| {
            sdk.get(key)
                .or_else(|| sdk["patch"].get(key))
                .and_then(Value::as_str)
        };
        let flag = sdk
            .get("is_backgrounded")
            .or_else(|| sdk["patch"].get("is_backgrounded"))
            .and_then(Value::as_bool);
        let id = clip(id, ID_BYTES);
        let index = match self.seen.iter().position(|(seen, ..)| *seen == id) {
            Some(index) => index,
            None => {
                if self.seen.len() == TRACKED_TASKS {
                    self.seen.pop_front();
                }
                self.seen.push_back((id, flag != Some(false), None));
                self.seen.len() - 1
            }
        };
        let (_, background, label) = &mut self.seen[index];
        *background |= flag == Some(true);
        if let Some(description) = text("description") {
            *label = Some(clip(description, SESSION_ACTIVITY_LABEL_BYTES));
        }
        background.then_some(label.as_deref())
    }
}

impl SdkBridge {
    pub(super) fn task(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let field = |key: &str| sdk.get(key).and_then(Value::as_str);
        let Some(id) = field("task_id") else {
            return;
        };
        let Some(label) = self.tasks.background(id, sdk) else {
            return;
        };
        let label = label.unwrap_or("Background task").to_owned();
        let status = field("status").or_else(|| sdk["patch"].get("status").and_then(Value::as_str));
        let state = match status {
            Some("completed") => SessionActivityState::Completed,
            Some("failed") => SessionActivityState::Failed,
            Some("stopped" | "killed") => SessionActivityState::Cancelled,
            _ => SessionActivityState::Running,
        };
        let progress = field("summary")
            .map(str::to_owned)
            .or_else(|| field("last_tool_name").map(|tool| format!("Using {tool}")))
            .or_else(|| {
                sdk["patch"]
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        if state.is_terminal() {
            self.tasks.ended = Some(label.clone());
        }
        let id = format!("{TASK_PREFIX}{id}");
        activity(
            inbound,
            &id,
            BACKGROUND_TASK,
            &label,
            state,
            progress.as_deref(),
        );
    }

    /// A run starts. One Claude Code started on its own, with no message of
    /// the user's, says why in its turn: usually a background task ended.
    pub(super) fn run_started(&mut self, own: bool, inbound: &mut Inbound) {
        let ended = self.tasks.ended.take();
        if !own {
            return;
        }
        self.tasks.own_runs += 1;
        let label = match ended {
            Some(task) => format!("Background task finished: {task}"),
            None => "Claude Code started this turn on its own".to_owned(),
        };
        let id = format!("run:{}", self.tasks.own_runs);
        let state = SessionActivityState::Completed;
        activity(inbound, &id, "harnessRun", &label, state, None);
    }
}
