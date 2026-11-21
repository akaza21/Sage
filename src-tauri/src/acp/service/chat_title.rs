//! Auxiliary work runs on the existing connection's event loop, never inside
//! a turn, journal or session-control lock and never on a waiting queue.
use sage_db::database::Database;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, HarnessAdapter},
        registry::TitleCapability,
    },
    runtime::{ProjectionPublisher, PumpIntent},
    store::title_attempt::{self, TitleAttempt, TitleClaim},
};

/// Shared by every lane of the application, with no waiting queue.
#[derive(Default)]
pub(crate) struct TitleAdmission {
    state: Mutex<AdmissionState>,
}
#[derive(Default)]
struct AdmissionState {
    busy: bool,
    foreground: u64,
    active_turns: usize,
}
impl TitleAdmission {
    fn state(&self) -> std::sync::MutexGuard<'_, AdmissionState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn foreground(&self) {
        self.state().foreground += 1;
    }
    fn turn(self: &Arc<Self>) -> ForegroundLease {
        let mut state = self.state();
        state.foreground += 1;
        state.active_turns += 1;
        ForegroundLease(Arc::clone(self))
    }
    fn generation(&self) -> u64 {
        self.state().foreground
    }
    fn acquire(self: &Arc<Self>) -> Option<(Lease, u64)> {
        let mut state = self.state();
        if state.busy || state.active_turns != 0 {
            return None;
        }
        state.busy = true;
        Some((Lease(Arc::clone(self)), state.foreground))
    }
}
pub(crate) struct ForegroundLease(Arc<TitleAdmission>);
impl Drop for ForegroundLease {
    fn drop(&mut self) {
        self.0.state().active_turns -= 1;
    }
}
struct Lease(Arc<TitleAdmission>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.state().busy = false;
    }
}

struct Active {
    attempt: Box<TitleAttempt>,
    deadline: Instant,
    foreground: u64,
    _lease: Lease,
    cancelled: bool,
    retry: bool,
}

fn failed_result(capability: TitleCapability, result: &Option<Option<String>>) -> bool {
    match result {
        Some(Some(raw)) => match capability {
            TitleCapability::ReadExisting => crate::chat_lifecycle::harness_title(raw).is_none(),
            _ => crate::chat_lifecycle::generated_title(raw).is_none(),
        },
        Some(None) => true,
        None => false,
    }
}

pub(crate) struct TitleScheduler {
    database: Database,
    publisher: Arc<dyn ProjectionPublisher>,
    capability: TitleCapability,
    admission: Arc<TitleAdmission>,
    active: Option<Active>,
    considered: bool,
}

impl TitleScheduler {
    pub(crate) fn foreground_turn(&self) -> ForegroundLease {
        self.admission.turn()
    }
    pub(crate) fn new(
        database: Database,
        publisher: Arc<dyn ProjectionPublisher>,
        capability: TitleCapability,
        admission: Arc<TitleAdmission>,
    ) -> Self {
        Self {
            database,
            publisher,
            capability,
            admission,
            active: None,
            considered: false,
        }
    }

    pub(crate) fn completed(&mut self, intent: &PumpIntent, adapter: &mut dyn HarnessAdapter) {
        if self.considered
            || self.active.is_some()
            || !matches!(
                self.capability,
                TitleCapability::ReadExisting | TitleCapability::GenerateIsolated
            )
        {
            return;
        }
        let owner = sage_acp_protocol::scheduler::RuntimeOwner {
            project_id: intent.project_id.clone(),
            chat_id: intent.chat_id.clone(),
            session_id: intent.session_id.clone(),
            runtime_epoch: intent.runtime_epoch,
        };
        self.start(owner, intent.active_turn.turn_id.clone(), adapter);
    }

    fn start(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        completed_turn: sage_acp_protocol::contract::SageTurnId,
        adapter: &mut dyn HarnessAdapter,
    ) {
        let Ok(mut connection) = self.database.write_connection() else {
            return;
        };
        let admission = self.admission.acquire();
        let mut attempt =
            match title_attempt::claim(&mut connection, owner, completed_turn, admission.is_some())
            {
                Ok(TitleClaim::Started(attempt)) => attempt,
                Ok(TitleClaim::Stopped) => {
                    self.considered = true;
                    return;
                }
                _ => return,
            };
        self.considered = true;
        drop(connection);
        tracing::info!("Auxiliary chat title attempt claimed");
        let Some((lease, foreground)) = admission else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        if let Err(error) = adapter.start_title(&attempt.input) {
            tracing::info!(?error, "Auxiliary chat title unavailable");
            return;
        }
        attempt.input = String::new();
        self.active = Some(Active {
            attempt,
            deadline,
            foreground,
            _lease: lease,
            cancelled: false,
            retry: false,
        });
    }

    pub(crate) fn pending(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn cancel(&mut self, adapter: &mut dyn HarnessAdapter) {
        if let Some(active) = &mut self.active {
            if !active.cancelled {
                tracing::info!("Auxiliary chat title preempted by foreground work");
                // Deadline/invalidated claims are terminal even if foreground
                // work arrives before the next scheduler poll.
                active.retry = Instant::now() < active.deadline
                    && title_attempt::valid(&self.database.connection(), &active.attempt)
                        .unwrap_or(false)
                    && !failed_result(self.capability, &adapter.take_title_result());
            }
            active.cancelled = true;
            adapter.cancel_title();
        }
    }

    pub(crate) fn poll(&mut self, adapter: &mut dyn HarnessAdapter) -> Option<AdapterEgress> {
        let active = self.active.as_mut()?;
        // Observe a real failure before deciding whether foreground preemption
        // is retryable. Adapters may complete asynchronously between polls.
        let prior_result = adapter.take_title_result();
        if !active.cancelled
            && (Instant::now() >= active.deadline
                || !title_attempt::valid(&self.database.connection(), &active.attempt)
                    .unwrap_or(false))
        {
            active.cancelled = true;
            adapter.cancel_title();
        }
        if !active.cancelled && active.foreground != self.admission.generation() {
            active.cancelled = true;
            active.retry = !failed_result(self.capability, &prior_result);
            adapter.cancel_title();
        }
        let egress = adapter.poll_title();
        let result = prior_result.or_else(|| adapter.take_title_result());
        if failed_result(self.capability, &result) {
            active.retry = false;
        }
        if let Some(Some(title)) = result
            && !active.cancelled
            && Instant::now() < active.deadline
            && let Ok(mut connection) = self.database.write_connection()
            && let Ok(Some(revision)) = match self.capability {
                TitleCapability::ReadExisting => {
                    title_attempt::apply_existing(&mut connection, &active.attempt, &title)
                }
                _ => title_attempt::apply(&mut connection, &active.attempt, &title),
            }
        {
            let _ = self
                .publisher
                .publish_committed(crate::acp::ipc::ProjectionChangedV1 {
                    project_id: active.attempt.owner.project_id.clone(),
                    chat_id: active.attempt.owner.chat_id.clone(),
                    session_id: active.attempt.owner.session_id.clone(),
                    projection_revision: revision,
                    child_projection_epoch: 0,
                });
        }
        let mut followup = None;
        if !adapter.title_pending() {
            if active.retry
                && !adapter.title_requires_teardown()
                && let Ok(mut connection) = self.database.write_connection()
                && title_attempt::preempt(&mut connection, &active.attempt).unwrap_or(false)
            {
                self.considered = false;
                followup = title_attempt::next_completion(&connection, &active.attempt)
                    .ok()
                    .flatten()
                    .map(|turn| (active.attempt.owner.clone(), turn));
            }
            self.active = None;
        }
        if let Some((owner, turn)) = followup {
            self.start(owner, turn, adapter);
        }
        egress
    }
}

#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) mod tests;
