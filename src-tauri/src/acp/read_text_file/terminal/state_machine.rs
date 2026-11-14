use super::*;
pub(crate) use settlement::lock;
use settlement::terminalize_locked;

pub(crate) fn insert_attempt(
    state: &ReadState,
    token: String,
    pending: PendingRead,
    notify: oneshot::Sender<()>,
) -> ReadTextResult<()> {
    let mut inner = lock(state);
    let session = owner_session_key(&pending.owner);
    let occupancy = inner
        .attempts
        .len()
        .checked_add(inner.receipts.len())
        .expect("bounded P-02 occupancy");
    if inner.reservations.contains_key(&session) || occupancy >= MAX_PENDING_READS {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Busy));
    }
    inner.reservations.insert(session, token.clone());
    inner.attempts.insert(
        token,
        Attempt {
            pending,
            classification: None,
            phase: AttemptPhase::Waiting,
            terminal: None,
            pending_terminal: None,
            cancellation: P02Cancellation::default(),
            notify: Some(notify),
        },
    );
    state.changed.notify_all();
    Ok(())
}

pub(crate) fn classify_attempt(
    state: &ReadState,
    request: ClassificationRequest<'_>,
) -> ReadTextResult<ReadTextNextAction> {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(request.token) else {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    };
    if attempt.phase != AttemptPhase::Waiting
        || attempt.classification.is_some()
        || &attempt.pending.owner != request.owner
        || &attempt.pending.resource != request.resource
        || attempt.pending.expected_editor_revision != request.expected_revision
        || request.durable_revision != request.expected_revision
        || Instant::now() >= attempt.pending.deadline
    {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    }
    let action = match (
        request.classification,
        request.editor_revision,
        request.expected_revision,
    ) {
        (ReadTextClassification::NotOpen, None, None) => ReadTextNextAction::DiskAllowed {
            state: DiskAllowedState::NotOpen,
            editor_revision: None,
        },
        (ReadTextClassification::OpenClean, Some(revision), None) if revision != 0 => {
            ReadTextNextAction::DiskAllowed {
                state: DiskAllowedState::OpenClean,
                editor_revision: Some(revision),
            }
        }
        (ReadTextClassification::Dirty, Some(revision), previous) if revision != 0 => {
            ReadTextNextAction::FlushRequired {
                editor_revision: revision,
                previous_durable_revision: previous,
            }
        }
        (ReadTextClassification::Persisted, Some(revision), Some(expected))
            if revision == expected && revision != 0 =>
        {
            ReadTextNextAction::Persisted {
                editor_revision: revision,
            }
        }
        _ => return Err(ReadTextFailure::terminal(ReadTextFailureKind::Corrupt)),
    };
    attempt.classification = Some(Classification {
        state: request.classification,
        editor_revision: request.editor_revision,
    });
    state.changed.notify_all();
    Ok(action)
}

pub(crate) fn acknowledge_start(
    state: &ReadState,
    owner: &ReadTextOwner,
    resource: &ResourceRef,
    token: &str,
    revision: Option<u64>,
    state_ack: &ReadTextFlushState,
) -> ReadTextResult<PendingRead> {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    };
    let Some(classification) = attempt.classification else {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    };
    let valid = match (
        classification.state,
        classification.editor_revision,
        state_ack,
    ) {
        (
            ReadTextClassification::NotOpen,
            None,
            ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::NotOpen,
                editor_revision: None,
            },
        ) => true,
        (
            ReadTextClassification::OpenClean,
            Some(actual),
            ReadTextFlushState::DiskAllowed {
                state: DiskAllowedState::OpenClean,
                editor_revision: Some(ack),
            },
        ) => actual == *ack,
        (
            ReadTextClassification::Dirty | ReadTextClassification::Persisted,
            Some(actual),
            ReadTextFlushState::Persisted {
                editor_revision,
                durable_revision,
            },
        ) => actual == *editor_revision && actual == *durable_revision,
        _ => false,
    };
    if attempt.phase != AttemptPhase::Waiting
        || &attempt.pending.owner != owner
        || &attempt.pending.resource != resource
        || attempt.pending.expected_editor_revision != revision
        || !valid
    {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    }
    if Instant::now() >= attempt.pending.deadline {
        let failure = ReadTextFailure::for_resource(
            ReadTextFailureKind::Timeout,
            &attempt.pending.resource,
            attempt.pending.range,
        );
        terminalize_locked(
            state,
            &mut inner,
            token,
            Terminal::Failure(Box::new(failure)),
        );
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    }
    attempt.phase = AttemptPhase::Reading;
    let pending = PendingRead {
        owner: attempt.pending.owner.clone(),
        resource: attempt.pending.resource.clone(),
        expected_editor_revision: attempt.pending.expected_editor_revision,
        workspace: attempt.pending.workspace.clone(),
        admission: attempt.pending.admission.clone(),
        range: attempt.pending.range,
        deadline: attempt.pending.deadline,
    };
    state.changed.notify_all();
    Ok(pending)
}

pub(crate) fn finish_read(
    state: &ReadState,
    token: &str,
    result: ReadTextResult<ReadTextCompletion>,
) {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get(token) else {
        return;
    };
    if attempt.phase != AttemptPhase::Reading {
        return;
    }
    let terminal = if Instant::now() >= attempt.pending.deadline {
        Terminal::Failure(Box::new(ReadTextFailure::for_resource(
            ReadTextFailureKind::Timeout,
            &attempt.pending.resource,
            attempt.pending.range,
        )))
    } else {
        match result {
            Ok(completion) => Terminal::Success(Box::new(completion)),
            Err(failure) => Terminal::Failure(failure),
        }
    };
    terminalize_locked(state, &mut inner, token, terminal);
}

pub(crate) fn cancel_exact(
    state: &ReadState,
    owner: &ReadTextOwner,
    token: &str,
) -> Option<Instant> {
    let mut inner = lock(state);
    let attempt = inner.attempts.get(token)?;
    if &attempt.pending.owner != owner
        || (attempt.phase == AttemptPhase::Revoked
            && !matches!(attempt.terminal, Some(Terminal::Success(_))))
    {
        return None;
    }
    let deadline = attempt.pending.deadline;
    let failure = ReadTextFailure::for_resource(
        ReadTextFailureKind::Cancelled,
        &attempt.pending.resource,
        attempt.pending.range,
    );
    terminalize_locked(
        state,
        &mut inner,
        token,
        Terminal::Failure(Box::new(failure)),
    );
    Some(deadline)
}
pub(crate) fn expire_all(state: &ReadState) {
    expire_before(state, Instant::now());
}
pub(crate) fn expire_at(state: &ReadState, now: Instant) {
    expire_before(state, now);
}
fn expire_before(state: &ReadState, now: Instant) {
    let mut inner = lock(state);
    let tokens: Vec<_> = inner
        .attempts
        .iter()
        .filter(|(_, attempt)| {
            attempt.pending.deadline <= now
                && !matches!(attempt.terminal, Some(Terminal::Failure(_)))
        })
        .map(|(token, _)| token.clone())
        .collect();
    for token in tokens {
        let attempt = inner.attempts.get(&token).expect("token exists");
        let failure = ReadTextFailure::for_resource(
            ReadTextFailureKind::Timeout,
            &attempt.pending.resource,
            attempt.pending.range,
        );
        terminalize_locked(
            state,
            &mut inner,
            &token,
            Terminal::Failure(Box::new(failure)),
        );
    }
}

mod settlement;
pub(crate) use settlement::{
    begin_delivery, claim_receiver, drain_receipts, lease_dropped, receiver_dropped, settle_lease,
    terminalize_all,
};
