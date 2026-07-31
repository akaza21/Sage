use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOperationKind {
    Read,
    Snapshot,
    Terminal,
}

#[derive(Debug, Default)]
struct CoordinatorInner {
    gate: Mutex<()>,
    next_ticket: AtomicU64,
    terminal_watermark: AtomicU64,
    project_transition_watermark: AtomicU64,
}

#[derive(Debug, Clone, Default)]
pub struct EditorOperationCoordinator {
    inner: Arc<CoordinatorInner>,
}

impl EditorOperationCoordinator {
    pub fn issue_ticket(&self) -> AppResult<u64> {
        self.inner
            .next_ticket
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |ticket| {
                ticket.checked_add(1)
            })
            .map(|previous| previous + 1)
            .map_err(|_| AppError::ResourceLimit("editor operation ticket space exhausted".into()))
    }

    pub fn execute<T>(
        &self,
        ticket: u64,
        kind: EditorOperationKind,
        operation: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        let _guard = self
            .inner
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ticket
            <= self
                .inner
                .project_transition_watermark
                .load(Ordering::Acquire)
        {
            return Err(AppError::InvalidRequest(
                "editor operation belongs to an earlier project transition".into(),
            ));
        }
        if kind == EditorOperationKind::Snapshot
            && ticket <= self.inner.terminal_watermark.load(Ordering::Acquire)
        {
            return Err(AppError::InvalidRequest(
                "editor snapshot was superseded by a save or discard".into(),
            ));
        }

        let result = operation();
        if kind == EditorOperationKind::Terminal {
            self.inner
                .terminal_watermark
                .fetch_max(ticket, Ordering::Release);
        }
        result
    }

    pub fn execute_project_transition<T>(
        &self,
        ticket: u64,
        operation: impl FnOnce() -> AppResult<T>,
    ) -> AppResult<T> {
        let _guard = self
            .inner
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = operation();
        if result.is_ok() {
            self.inner
                .project_transition_watermark
                .fetch_max(ticket, Ordering::Release);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_operations_reject_older_snapshots() {
        let coordinator = EditorOperationCoordinator::default();
        let snapshot_ticket = coordinator.issue_ticket().unwrap();
        let terminal_ticket = coordinator.issue_ticket().unwrap();

        coordinator
            .execute(terminal_ticket, EditorOperationKind::Terminal, || Ok(()))
            .unwrap();
        assert!(
            coordinator
                .execute(snapshot_ticket, EditorOperationKind::Snapshot, || Ok(()))
                .is_err()
        );
    }

    #[test]
    fn failed_terminal_attempts_still_reject_older_snapshots() {
        let coordinator = EditorOperationCoordinator::default();
        let snapshot_ticket = coordinator.issue_ticket().unwrap();
        let terminal_ticket = coordinator.issue_ticket().unwrap();

        assert!(
            coordinator
                .execute(terminal_ticket, EditorOperationKind::Terminal, || {
                    Err::<(), _>(AppError::InvalidRequest("injected terminal failure".into()))
                })
                .is_err()
        );
        assert!(
            coordinator
                .execute(snapshot_ticket, EditorOperationKind::Snapshot, || Ok(()))
                .is_err()
        );
    }

    #[test]
    fn successful_project_transition_rejects_all_older_editor_operations() {
        let coordinator = EditorOperationCoordinator::default();
        let old_read = coordinator.issue_ticket().unwrap();
        let transition = coordinator.issue_ticket().unwrap();
        coordinator
            .execute_project_transition(transition, || Ok(()))
            .unwrap();
        assert!(
            coordinator
                .execute(old_read, EditorOperationKind::Read, || Ok(()))
                .is_err()
        );
    }
}
