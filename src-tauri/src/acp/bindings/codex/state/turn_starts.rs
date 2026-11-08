use crate::acp::{contract::RequiredText, intent::ExternalId, limits::LIMITS};

use super::{AdapterError, CodexAdapter};
use crate::acp::intent::{NormalizedPayload, session_event::SessionScopedEvent};

/// Thread-less notices held before a thread is bound.
pub(crate) const PENDING_NOTICES_MAX: usize = 4;

impl CodexAdapter {
    pub(crate) fn learn_bound_child(&mut self, child: &str) {
        if self
            .bound_child_threads
            .iter()
            .any(|known| known.as_str() == child)
        {
            return;
        }
        if self.bound_child_threads.len() == 32 {
            self.bound_child_threads.remove(0);
        }
        if let Ok(child) = crate::acp::contract::SafeText::new("childThreadId", child) {
            self.bound_child_threads.push(child);
        }
    }

    pub(crate) fn is_bound_child_thread(&self, thread: &str) -> bool {
        self.goal_supported()
            && self
                .bound_child_threads
                .iter()
                .any(|known| known.as_str() == thread)
    }

    pub(crate) fn learn_child_threads(&mut self, payload: &NormalizedPayload) {
        let NormalizedPayload::Collaboration {
            operation,
            agent_key,
            agent_keys,
            ..
        } = payload
        else {
            return;
        };
        let keys = match operation.as_ref().map(|value| value.as_str()) {
            Some("spawnAgent") => agent_keys.as_slice(),
            Some("started" | "interacted" | "interrupted") => {
                agent_key.as_ref().map(std::slice::from_ref).unwrap_or(&[])
            }
            _ => &[],
        };
        for key in keys {
            if self
                .external_thread_id
                .as_ref()
                .is_some_and(|parent| parent.as_str() == key.as_str())
                || self.child_threads.iter().any(|known| known == key)
            {
                continue;
            }
            if self.child_threads.len() == 32 {
                self.child_threads.remove(0);
            }
            self.child_threads.push(key.clone());
        }
    }

    pub(crate) fn is_child_thread(&self, thread: &str) -> bool {
        self.child_threads
            .iter()
            .any(|known| known.as_str() == thread)
    }

    pub(crate) fn ignore_unbound_thread_notification(&mut self, thread: &str) -> bool {
        if self
            .external_thread_id
            .as_ref()
            .is_some_and(|parent| parent.as_str() == thread)
        {
            return false;
        }
        if self.is_child_thread(thread) || self.is_fork_thread(thread) {
            return true;
        }
        if self.child_threads.is_empty() {
            return false;
        }
        if !self.unknown_child_notification_reported {
            if super::super::incoming::reports_diagnostics() {
                tracing::warn!("Codex unbound-thread notification ignored after subagent activity");
            }
            self.unknown_child_notification_reported = true;
        }
        true
    }

    pub(crate) fn buffer_browser_mcp_failure(&mut self, thread: &str) {
        if self.browser_mcp_failure_reported
            || self
                .pending_browser_mcp_failure_threads
                .iter()
                .any(|known| known == thread)
        {
            return;
        }
        if self.pending_browser_mcp_failure_threads.len() == 8 {
            self.pending_browser_mcp_failure_threads.remove(0);
        }
        self.pending_browser_mcp_failure_threads
            .push(thread.to_owned());
    }

    /// Holds a thread-less notice until a thread is bound; the oldest is
    /// dropped at the cap.
    pub(crate) fn buffer_session_notice(&mut self, notice: SessionScopedEvent) {
        if self.pending_session_notices.len() == PENDING_NOTICES_MAX {
            self.pending_session_notices.remove(0);
        }
        self.pending_session_notices.push(notice);
    }

    pub(crate) fn take_session_notices(&mut self) -> Vec<SessionScopedEvent> {
        std::mem::take(&mut self.pending_session_notices)
    }

    pub(crate) fn take_browser_mcp_failure_for_thread(&mut self, thread: &str) -> bool {
        let matched = std::mem::take(&mut self.pending_browser_mcp_failure_threads)
            .iter()
            .any(|pending| pending == thread);
        matched && self.mark_browser_mcp_failure_reported()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum CorrelationState {
    Pending,
    Active(ExternalId),
    Terminal(ExternalId),
}

#[derive(Clone, Debug, PartialEq)]
struct TurnStartCorrelation {
    request_id: u64,
    client_message_id: RequiredText<512>,
    state: CorrelationState,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TurnStartCorrelations {
    entries: Vec<TurnStartCorrelation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CorrelationMatch {
    Pending(RequiredText<512>),
    Active {
        turn_id: ExternalId,
        client_message_id: RequiredText<512>,
    },
    Terminal {
        turn_id: ExternalId,
        client_message_id: RequiredText<512>,
    },
}

impl TurnStartCorrelations {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(super) fn has_pending(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| matches!(entry.state, CorrelationState::Pending))
    }

    pub(super) fn is_pending(&self, request_id: u64) -> bool {
        self.entries.iter().any(|entry| {
            entry.request_id == request_id && matches!(entry.state, CorrelationState::Pending)
        })
    }

    /// The request whose correlation the Turn bound, if any.
    pub(super) fn request_for(&self, turn_id: &ExternalId) -> Option<u64> {
        self.entries.iter().find_map(|entry| {
            matches!(&entry.state, CorrelationState::Active(active) if active == turn_id)
                .then_some(entry.request_id)
        })
    }

    pub(super) fn register(&mut self, request_id: u64, client_message_id: RequiredText<512>) {
        debug_assert!(self.entries.len() < LIMITS.queues.outstanding_rpc_per_session as usize);
        debug_assert!(
            !self
                .entries
                .iter()
                .any(|entry| entry.request_id == request_id)
        );
        debug_assert!(!self.has_pending());
        self.entries.push(TurnStartCorrelation {
            request_id,
            client_message_id,
            state: CorrelationState::Pending,
        });
    }

    pub(super) fn observe(
        &mut self,
        turn_id: &ExternalId,
    ) -> Result<CorrelationMatch, AdapterError> {
        if let Some(known) = self.known_turn(turn_id) {
            return Ok(known);
        }
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| matches!(entry.state, CorrelationState::Pending))
            .ok_or(AdapterError::TurnIntentConflict)?;
        entry.state = CorrelationState::Active(turn_id.clone());
        Ok(CorrelationMatch::Pending(entry.client_message_id.clone()))
    }

    pub(super) fn response(
        &self,
        request_id: u64,
        turn_id: &ExternalId,
    ) -> Result<CorrelationMatch, AdapterError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.request_id == request_id)
            .ok_or(AdapterError::TurnIntentConflict)?;
        match &entry.state {
            CorrelationState::Pending => {
                if self.entries.iter().any(|other| {
                    other.request_id != request_id
                        && match &other.state {
                            CorrelationState::Active(known) | CorrelationState::Terminal(known) => {
                                known == turn_id
                            }
                            CorrelationState::Pending => false,
                        }
                }) {
                    return Err(AdapterError::IdentityConflict);
                }
                Ok(CorrelationMatch::Pending(entry.client_message_id.clone()))
            }
            CorrelationState::Active(known) if known == turn_id => Ok(CorrelationMatch::Active {
                turn_id: known.clone(),
                client_message_id: entry.client_message_id.clone(),
            }),
            CorrelationState::Terminal(known) if known == turn_id => {
                Ok(CorrelationMatch::Terminal {
                    turn_id: known.clone(),
                    client_message_id: entry.client_message_id.clone(),
                })
            }
            CorrelationState::Active(_) | CorrelationState::Terminal(_) => {
                Err(AdapterError::IdentityConflict)
            }
        }
    }

    pub(super) fn failure(&self, request_id: u64) -> Result<CorrelationMatch, AdapterError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.request_id == request_id)
            .ok_or(AdapterError::TurnIntentConflict)?;
        Ok(match &entry.state {
            CorrelationState::Pending => CorrelationMatch::Pending(entry.client_message_id.clone()),
            CorrelationState::Active(turn_id) => CorrelationMatch::Active {
                turn_id: turn_id.clone(),
                client_message_id: entry.client_message_id.clone(),
            },
            CorrelationState::Terminal(turn_id) => CorrelationMatch::Terminal {
                turn_id: turn_id.clone(),
                client_message_id: entry.client_message_id.clone(),
            },
        })
    }

    pub(super) fn discard_pending(&mut self, request_id: u64) -> Result<(), AdapterError> {
        let index = self
            .entries
            .iter()
            .position(|entry| {
                entry.request_id == request_id && matches!(entry.state, CorrelationState::Pending)
            })
            .ok_or(AdapterError::TurnIntentConflict)?;
        self.entries.swap_remove(index);
        Ok(())
    }

    pub(super) fn consume(&mut self, request_id: u64) -> Result<(), AdapterError> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.request_id == request_id)
            .ok_or(AdapterError::TurnIntentConflict)?;
        self.entries.swap_remove(index);
        Ok(())
    }

    pub(super) fn mark_terminal(&mut self, turn_id: &ExternalId) {
        if let Some(entry) = self.entries.iter_mut().find(
            |entry| matches!(&entry.state, CorrelationState::Active(active) if active == turn_id),
        ) {
            entry.state = CorrelationState::Terminal(turn_id.clone());
        }
    }

    fn known_turn(&self, turn_id: &ExternalId) -> Option<CorrelationMatch> {
        self.entries.iter().find_map(|entry| match &entry.state {
            CorrelationState::Active(known) if known == turn_id => Some(CorrelationMatch::Active {
                turn_id: known.clone(),
                client_message_id: entry.client_message_id.clone(),
            }),
            CorrelationState::Terminal(known) if known == turn_id => {
                Some(CorrelationMatch::Terminal {
                    turn_id: known.clone(),
                    client_message_id: entry.client_message_id.clone(),
                })
            }
            CorrelationState::Pending
            | CorrelationState::Active(_)
            | CorrelationState::Terminal(_) => None,
        })
    }
}
