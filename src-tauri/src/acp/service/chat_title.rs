//! Auxiliary work runs on the existing connection's event loop, never inside
//! a turn, journal or session-control lock and never on a waiting queue.
use sage_db::database::Database;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, HarnessAdapter},
        registry::TitleCapability,
    },
    runtime::{ProjectionPublisher, PumpIntent},
    store::title_attempt::{self, TitleAttempt},
};

/// Shared by every lane of the application, with no waiting queue.
#[derive(Default)]
pub(crate) struct TitleAdmission {
    busy: AtomicBool,
    foreground: AtomicU64,
}
impl TitleAdmission {
    pub(crate) fn foreground(&self) {
        self.foreground.fetch_add(1, Ordering::AcqRel);
    }
    fn acquire(self: &Arc<Self>) -> Option<Lease> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Lease(Arc::clone(self)))
    }
}
struct Lease(Arc<TitleAdmission>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.busy.store(false, Ordering::Release);
    }
}

struct Active {
    attempt: TitleAttempt,
    deadline: Instant,
    foreground: u64,
    _lease: Lease,
    cancelled: bool,
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
        let Ok(mut connection) = self.database.write_connection() else {
            return;
        };
        let Ok(Some(mut attempt)) =
            title_attempt::claim(&mut connection, owner, intent.active_turn.turn_id.clone())
        else {
            return;
        };
        self.considered = true;
        drop(connection);
        tracing::info!("Auxiliary chat title attempt claimed");
        // Even busy admission spends the durable attempt. There is no later retry.
        let Some(lease) = self.admission.acquire() else {
            tracing::info!("Auxiliary chat title skipped: capacity busy");
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        let foreground = self.admission.foreground.load(Ordering::Acquire);
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
        });
    }

    pub(crate) fn pending(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn cancel(&mut self, adapter: &mut dyn HarnessAdapter) {
        if let Some(active) = &mut self.active {
            if !active.cancelled {
                tracing::info!("Auxiliary chat title preempted by foreground work");
            }
            active.cancelled = true;
            adapter.cancel_title();
        }
    }

    pub(crate) fn poll(&mut self, adapter: &mut dyn HarnessAdapter) -> Option<AdapterEgress> {
        let active = self.active.as_mut()?;
        if !active.cancelled
            && (Instant::now() >= active.deadline
                || active.foreground != self.admission.foreground.load(Ordering::Acquire)
                || !title_attempt::valid(&self.database.connection(), &active.attempt)
                    .unwrap_or(false))
        {
            active.cancelled = true;
            adapter.cancel_title();
        }
        let egress = adapter.poll_title();
        if let Some(Some(title)) = adapter.take_title_result()
            && !active.cancelled
            && Instant::now() < active.deadline
            && let Ok(mut connection) = self.database.write_connection()
            && let Ok(Some(revision)) =
                title_attempt::apply(&mut connection, &active.attempt, &title)
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
        if !adapter.title_pending() {
            self.active = None;
        }
        egress
    }
}

#[cfg(all(test, feature = "acp-fixture"))]
mod tests;
