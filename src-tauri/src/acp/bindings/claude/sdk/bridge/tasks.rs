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
/// A background subagent's task: Resources lists it under Subagents.
const SUBAGENT: &str = "subagent";
/// Tasks remembered, oldest dropped first; a task's last report drops it.
const TRACKED_TASKS: usize = 64;
const ID_BYTES: usize = 256;

#[derive(Debug, Clone)]
struct Task {
    id: String,
    background: bool,
    subagent: bool,
    label: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct Tasks {
    seen: VecDeque<Task>,
    /// The latest background task to end since Claude Code last ran.
    ended: Option<String>,
    /// Runs Claude Code started on its own, numbering their rows.
    own_runs: u64,
}

impl Tasks {
    /// The task this event belongs to, when it runs in the background.
    /// `task_started` says whether it starts there (a CLI that does not say
    /// is taken to), and `task_updated` can move it there. `task_notification`
    /// is a task's last report, so it forgets the task.
    fn background(&mut self, id: &str, sdk: &Value) -> Option<Task> {
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
        let index = match self.seen.iter().position(|task| task.id == id) {
            Some(index) => index,
            None => {
                if self.seen.len() == TRACKED_TASKS {
                    self.seen.pop_front();
                }
                self.seen.push_back(Task {
                    id,
                    background: flag != Some(false),
                    subagent: false,
                    label: None,
                });
                self.seen.len() - 1
            }
        };
        let task = &mut self.seen[index];
        task.background |= flag == Some(true);
        task.subagent |= text("subagent_type").is_some();
        if let Some(description) = text("description") {
            task.label = Some(clip(description, SESSION_ACTIVITY_LABEL_BYTES));
        }
        let task = if sdk.get("subtype").and_then(Value::as_str) == Some("task_notification") {
            self.seen.remove(index)?
        } else {
            self.seen[index].clone()
        };
        task.background.then_some(task)
    }
}

impl SdkBridge {
    pub(super) fn task(&mut self, sdk: &Value, inbound: &mut Inbound) {
        let field = |key: &str| sdk.get(key).and_then(Value::as_str);
        let Some(id) = field("task_id") else {
            return;
        };
        let Some(task) = self.tasks.background(id, sdk) else {
            return;
        };
        let label = task.label.as_deref().unwrap_or("Background task");
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
        // A stopped task did not finish; only a finished one explains a run.
        if matches!(
            state,
            SessionActivityState::Completed | SessionActivityState::Failed
        ) {
            self.tasks.ended = Some(label.to_owned());
        }
        let kind = if task.subagent {
            SUBAGENT
        } else {
            BACKGROUND_TASK
        };
        let id = format!("{TASK_PREFIX}{id}");
        activity(inbound, &id, kind, label, state, progress.as_deref());
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
