//! Auxiliary work runs on the existing connection's event loop, never inside
//! a turn, journal or session-control lock and never on a waiting queue.
use sage_db::database::Database;
use std::{
    sync::{Arc, atomic::Ordering},
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

mod admission;
pub(crate) use admission::{ForegroundLease, TitleAdmission};
use admission::{Lease, Wake};

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
    wake: Option<Wake>,
    suppress_wake: bool,
    wake_checked: bool,
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
            wake: None,
            suppress_wake: false,
            wake_checked: false,
        }
    }

    pub(crate) fn bind_wakeup(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        mailbox: &Arc<crate::acp::runtime::DriverMailbox>,
    ) {
        if !matches!(
            self.capability,
            TitleCapability::ReadExisting | TitleCapability::GenerateIsolated
        ) {
            return;
        }
        if self.wake.is_some() {
            return;
        }
        self.wake = self
            .admission
            .register(self.database.clone(), owner, mailbox);
        self.admission.wake_pending();
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
        // Drop the database guard before the lease: releasing admission can
        // read the next pending marker from the same connection.
        let admission = self.admission.acquire(&owner);
        self.record_completion(owner, completed_turn, admission, adapter);
    }

    fn record_completion(
        &mut self,
        owner: sage_acp_protocol::scheduler::RuntimeOwner,
        completed_turn: sage_acp_protocol::contract::SageTurnId,
        admission: Option<(Lease, u64)>,
        adapter: &mut dyn HarnessAdapter,
    ) {
        let Ok(mut connection) = self.database.write_connection() else {
            return;
        };
        let attempt =
            match title_attempt::claim(&mut connection, owner, completed_turn, admission.is_some())
            {
                Ok(TitleClaim::Started(attempt)) => attempt,
                Ok(TitleClaim::Stopped) => {
                    self.considered = true;
                    return;
                }
                Ok(TitleClaim::Pending) => {
                    // Admission may have become free before this marker was
                    // committed, after the release wakeup already queried it.
                    drop(connection);
                    self.admission.wake_pending();
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
        self.launch(attempt, lease, foreground, adapter);
    }

    fn launch(
        &mut self,
        mut attempt: Box<TitleAttempt>,
        lease: Lease,
        foreground: u64,
        adapter: &mut dyn HarnessAdapter,
    ) {
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
        self.suppress_wake = true;
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
        // Binding precedes durable activation. The first idle poll runs after
        // activation commits, so an older epoch's pending marker can wake now.
        if !self.wake_checked && self.wake.is_some() {
            self.wake_checked = true;
            self.admission.wake_pending();
        }
        if !std::mem::take(&mut self.suppress_wake)
            && self.active.is_none()
            && let Some(wake) = &self.wake
            && wake.ready.swap(false, Ordering::AcqRel)
        {
            let owner = wake.owner.clone();
            let admission = self.admission.acquire(&owner);
            if let Some((lease, foreground)) = admission {
                let attempt = self
                    .database
                    .write_connection()
                    .ok()
                    .and_then(|mut connection| title_attempt::retry(&mut connection, owner).ok());
                if let Some(TitleClaim::Started(attempt)) = attempt {
                    self.considered = true;
                    self.launch(attempt, lease, foreground, adapter);
                }
            }
        }
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

impl Drop for TitleScheduler {
    fn drop(&mut self) {
        if let Some(wake) = &self.wake {
            self.admission.unregister(&wake.owner);
        }
    }
}

#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) mod tests;
