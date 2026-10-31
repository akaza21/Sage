use super::*;
use crate::acp::contract::{SageChatId, SageProjectId};

#[derive(Debug)]
pub(super) struct InFlight {
    pub(super) acknowledgement: Arc<Acknowledgement>,
    mutations: Arc<Vec<StoreMutation>>,
    _permit: tokio::sync::SemaphorePermit<'static>,
}

pub(super) type Prepare = dyn Fn(&crate::persistence::Database, &[StoreMutation], Duration) -> Result<(), StoreError>
    + Send
    + Sync;

/// Retain the envelope's existing journal reservation while preparing optional
/// Git evidence away from the shared actor. No worker queue: saturation skips
/// the optional capture and commits the turn normally.
// Reserve a short commit window after bounded Git evidence collection.
const TERMINAL_COMMIT_RESERVE_MS: u64 = 500;

pub(super) fn defer(
    queue: &Arc<Queue>,
    envelope: Envelope,
    prepare: &Arc<Prepare>,
) -> Option<Envelope> {
    static WORKERS: tokio::sync::Semaphore =
        tokio::sync::Semaphore::const_new(crate::turn_workspace::MAX_CONCURRENT_CAPTURES);
    if envelope.workspace_result.is_some()
        || envelope.workspace_database.is_none()
        || !crate::turn_workspace::needs_terminal_capture(&envelope.mutations)
    {
        return Some(envelope);
    }
    let Ok(permit) = WORKERS.try_acquire() else {
        return Some(envelope);
    };
    let prepare = Arc::clone(prepare);
    dispatch(queue, envelope, permit, move |envelope| {
        let budget = Duration::from_millis(
            envelope
                .deadline_millis
                .saturating_sub(now_millis())
                .saturating_sub(TERMINAL_COMMIT_RESERVE_MS)
                .min(crate::turn_workspace::CAPTURE_BUDGET_MILLIS),
        );
        prepare(
            envelope
                .workspace_database
                .as_ref()
                .expect("capture database"),
            &envelope.mutations,
            budget,
        )
    });
    None
}

fn dispatch(
    queue: &Arc<Queue>,
    mut envelope: Envelope,
    permit: tokio::sync::SemaphorePermit<'static>,
    prepare: impl FnOnce(&Envelope) -> Result<(), StoreError> + Send + 'static,
) {
    let mut state = lock(queue);
    if state.aborted {
        envelope.acknowledgement.close();
        drop(state);
        return;
    }
    state.workspace_inflight.push(InFlight {
        acknowledgement: Arc::clone(&envelope.acknowledgement),
        mutations: Arc::clone(&envelope.mutations),
        _permit: permit,
    });
    drop(state);
    let queue = Arc::clone(queue);
    tauri::async_runtime::spawn_blocking(move || {
        envelope.workspace_result = Some(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(&envelope)))
                .unwrap_or(Err(StoreError::Corrupt("workspace capture worker"))),
        );
        let mut state = lock(&queue);
        if state.aborted {
            drop(state);
            discard(&envelope);
            let acknowledgement = Arc::clone(&envelope.acknowledgement);
            drop(envelope);
            lock(&queue)
                .workspace_inflight
                .retain(|entry| !Arc::ptr_eq(&entry.acknowledgement, &acknowledgement));
        } else {
            match envelope
                .reservation
                .as_ref()
                .expect("retained capture reservation")
                .lane
            {
                JournalLane::Data => state.data.push_back(envelope),
                JournalLane::Control => state.control.push(envelope),
            }
            drop(state);
        }
        queue.ready.notify_all();
    });
}

pub(super) fn resume(state: &mut QueueState, envelope: &Envelope) {
    if envelope.workspace_result.is_some() {
        state
            .workspace_inflight
            .retain(|entry| !Arc::ptr_eq(&entry.acknowledgement, &envelope.acknowledgement));
    }
}

pub(super) fn blocked(state: &QueueState, envelope: &Envelope) -> bool {
    state.workspace_inflight.iter().any(|capture| {
        !Arc::ptr_eq(&capture.acknowledgement, &envelope.acknowledgement)
            && capture.mutations.iter().any(|left| {
                envelope
                    .mutations
                    .iter()
                    .any(|right| match (owner(left), owner(right)) {
                        (Some(left), Some(right)) => left == right,
                        _ => true,
                    })
            })
    })
}

fn owner(mutation: &StoreMutation) -> Option<(&SageProjectId, &SageChatId)> {
    match mutation {
        StoreMutation::LookupTurn(c) | StoreMutation::PrepareTurn(c) => {
            Some((&c.project_id, &c.chat_id))
        }
        StoreMutation::LookupActiveTurn(c)
        | StoreMutation::BeginSend(c)
        | StoreMutation::ObserveSend { command: c, .. }
        | StoreMutation::TurnTerminal { command: c, .. } => Some((&c.project_id, &c.chat_id)),
        StoreMutation::BeginCancel(c) => Some((&c.turn.project_id, &c.turn.chat_id)),
        StoreMutation::StartSession(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::ActivateSession(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::AbortActivation(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::CompleteActivation(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::FailActivation(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::CloseSession(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::FinalizeClose(c) => Some((&c.request.project_id, &c.request.chat_id)),
        StoreMutation::ParkSession(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::ReceivePermission { intent: c, .. } => Some((&c.project_id, &c.chat_id)),
        StoreMutation::RequestState(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::ItemIngress(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::ActivityIngress(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::AdapterInboundBatch(c) => Some((&c.owner().project_id, &c.owner().chat_id)),
        StoreMutation::RecoverRuntimeAfterZero(c) => Some((&c.project_id, &c.chat_id)),
        StoreMutation::RecoverAfterRestart { .. } => None,
    }
}

pub(super) fn discard(envelope: &Envelope) {
    if envelope.workspace_result.is_some() {
        crate::turn_workspace::discard_rejected_staging(
            envelope.workspace_database.as_ref(),
            &envelope.mutations,
            &Err(StoreError::Conflict {
                resource: "expired workspace capture",
            }),
        );
    }
}

#[cfg(test)]
mod tests;
