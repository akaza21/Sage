//! Ephemeral native Harness Goal state. No Goal is inferred from chat text.
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};

use super::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    scheduler::RuntimeOwner,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GoalAction {
    PauseExisting,
    ClearExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeRpcIntent {
    pub(crate) owner: RuntimeOwner,
    pub(crate) request: NativeRpcRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeRpcRequest {
    Goal(GoalAction),
    ChildThreadRead {
        child_thread_id: String,
        parent_thread_id: String,
    },
    /// Forks the runtime's own native session through its latest turn. Only
    /// an idle runtime whose negotiated capabilities admit
    /// `nativeSessionFork` accepts it.
    ForkSession,
    /// Releases a fork this runtime created once its fork Chat was committed
    /// or refused. Best effort: the fork Chat never depends on it.
    ReleaseFork {
        fork: crate::acp::intent::ExternalId,
        committed: bool,
    },
    /// Refreshes the runtime's usage-limit snapshot from the harness.
    UsageRead,
    /// Acts on the running session without starting a turn.
    Control(crate::acp::session_control::SessionControl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NativeRpcReply {
    GoalApplied,
    ChildTranscript(crate::acp::bindings::codex::ChildTranscriptSnapshot),
    TooLarge,
    Rejected,
    /// The harness created a new native session holding the forked history.
    Forked(crate::acp::intent::ExternalId),
    /// The harness let go of a fork this runtime created.
    Released,
    /// The harness answered a usage read; the snapshot is in the usage cell.
    UsageRead,
    /// The harness carried out a session control; the text says what it did.
    Controlled(Option<crate::acp::contract::SafeText<512>>),
    /// The harness declined a session control, with its reason if it gave one.
    ControlRefused(Option<crate::acp::contract::SafeText<512>>),
}

impl NativeRpcIntent {
    /// A read or a cosmetic change nothing waits on beyond its caller: it
    /// gets a short deadline, and missing it abandons the request instead of
    /// recovering the runtime.
    pub(crate) fn passive(&self) -> bool {
        matches!(
            self.request,
            NativeRpcRequest::UsageRead
                | NativeRpcRequest::Control(
                    crate::acp::session_control::SessionControl::SetTitle { .. }
                )
        )
    }

    pub(crate) fn actual_bytes(&self) -> usize {
        self.owner.project_id.as_str().len()
            + self.owner.chat_id.as_str().len()
            + self.owner.session_id.as_str().len()
            + std::mem::size_of::<RuntimeEpoch>()
            + match &self.request {
                NativeRpcRequest::Goal(_) => 1,
                NativeRpcRequest::ChildThreadRead {
                    child_thread_id,
                    parent_thread_id,
                } => child_thread_id.len() + parent_thread_id.len() + 1,
                NativeRpcRequest::ForkSession | NativeRpcRequest::UsageRead => 1,
                NativeRpcRequest::ReleaseFork { fork, .. } => fork.as_str().len() + 1,
                NativeRpcRequest::Control(control) => control.actual_bytes() + 1,
            }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum GoalState {
    Unsupported,
    Offline,
    Loading,
    Unavailable,
    NoGoal,
    Goal { snapshot: GoalSnapshot },
}

impl GoalState {
    #[cfg(test)]
    pub(crate) fn availability(&self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Offline => "offline",
            Self::Loading => "loading",
            Self::Unavailable => "unavailable",
            Self::NoGoal => "noGoal",
            Self::Goal { .. } => "goal",
        }
    }

    #[cfg(test)]
    pub(crate) fn objective(&self) -> Option<&str> {
        match self {
            Self::Goal { snapshot } => Some(&snapshot.objective),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GoalSnapshot {
    pub(crate) objective: String,
    pub(crate) status: GoalStatus,
    pub(crate) token_budget: Option<u64>,
    pub(crate) tokens_used: u64,
    pub(crate) time_used_seconds: u64,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum GoalStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}

impl GoalSnapshot {
    pub(crate) fn from_wire(value: &Value) -> Result<Self, String> {
        let object = value.as_object().ok_or("goal object missing")?;
        let objective = object
            .get("objective")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.chars().count() <= 4_000 && !s.contains('\0'))
            .ok_or("goal objective invalid")?;
        let status = match object.get("status").and_then(Value::as_str) {
            Some("active") => GoalStatus::Active,
            Some("paused") => GoalStatus::Paused,
            Some("blocked") => GoalStatus::Blocked,
            Some("usageLimited") => GoalStatus::UsageLimited,
            Some("budgetLimited") => GoalStatus::BudgetLimited,
            Some("complete") => GoalStatus::Complete,
            _ => return Err("goal status invalid".into()),
        };
        let number = |key: &str| -> Result<u64, String> {
            object
                .get(key)
                .and_then(Value::as_u64)
                .filter(|value| *value <= 9_007_199_254_740_991)
                .ok_or_else(|| format!("goal {key} invalid"))
        };
        let timestamp = |key: &str| -> Result<Option<String>, String> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value))
                    if value.len() <= 64 && !value.chars().any(char::is_control) =>
                {
                    Ok(Some(value.clone()))
                }
                _ => Err(format!("goal {key} invalid")),
            }
        };
        let token_budget = match object.get("tokenBudget") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_u64()
                    .filter(|value| *value <= 9_007_199_254_740_991)
                    .ok_or("goal tokenBudget invalid")?,
            ),
        };
        Ok(Self {
            objective: objective.to_owned(),
            status,
            token_budget,
            tokens_used: number("tokensUsed")?,
            time_used_seconds: number("timeUsedSeconds")?,
            created_at: timestamp("createdAt")?,
            updated_at: timestamp("updatedAt")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GoalChangedV1 {
    pub(crate) schema_version: u8,
    pub(crate) sage_project_id: SageProjectId,
    pub(crate) sage_chat_id: SageChatId,
    pub(crate) sage_agent_session_id: SageAgentSessionId,
    pub(crate) runtime_epoch: RuntimeEpoch,
}

pub(crate) type GoalNoticeSink = Arc<dyn Fn(GoalChangedV1) + Send + Sync>;

struct Entry {
    owner: RuntimeOwner,
    live: bool,
    state: GoalState,
}

#[derive(Default)]
pub(crate) struct GoalChannel {
    entry: Mutex<Option<Entry>>,
    sink: Mutex<Option<GoalNoticeSink>>,
}

impl GoalChannel {
    pub(crate) fn set_sink(&self, sink: GoalNoticeSink) {
        *self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sink);
    }

    pub(crate) fn reserve(&self, owner: RuntimeOwner) {
        *self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Entry {
            owner,
            live: false,
            state: GoalState::Loading,
        });
    }

    pub(crate) fn publish(&self, owner: &RuntimeOwner) {
        let mut entry = self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let matched = if let Some(current) = entry.as_mut().filter(|e| &e.owner == owner) {
            current.live = true;
            true
        } else {
            false
        };
        drop(entry);
        if matched {
            self.notify(owner);
        }
    }

    pub(crate) fn update(&self, owner: &RuntimeOwner, state: GoalState) {
        let mut entry = self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = match entry.as_mut().filter(|e| &e.owner == owner) {
            Some(current) if current.state != state => {
                current.state = state;
                current.live
            }
            _ => false,
        };
        drop(entry);
        if changed {
            self.notify(owner);
        }
    }

    pub(crate) fn snapshot(&self, owner: &RuntimeOwner) -> Option<GoalState> {
        self.entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|e| e.live && &e.owner == owner)
            .map(|e| e.state.clone())
    }

    pub(crate) fn clear(&self, owner: &RuntimeOwner) {
        let mut entry = self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entry.as_ref().is_some_and(|e| &e.owner == owner) {
            *entry = None;
        }
    }

    pub(crate) fn clear_all(&self) {
        *self
            .entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn notify(&self, owner: &RuntimeOwner) {
        let sink = self
            .sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(sink) = sink {
            sink(GoalChangedV1 {
                schema_version: 1,
                sage_project_id: owner.project_id.clone(),
                sage_chat_id: owner.chat_id.clone(),
                sage_agent_session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
            });
        }
    }
}
