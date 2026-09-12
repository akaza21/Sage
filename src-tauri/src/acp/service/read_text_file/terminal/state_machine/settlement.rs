use super::*;
use crate::acp::transport::P02EgressOutcome;

pub(crate) fn terminalize_all(
    state: &ReadState,
    owner: Option<&ReadTextOwner>,
) -> std::collections::BTreeSet<String> {
    let mut inner = lock(state);
    let tokens: std::collections::BTreeSet<_> = inner
        .attempts
        .iter()
        .filter(|(_, attempt)| owner.is_none_or(|wanted| wanted == &attempt.pending.owner))
        .map(|(token, _)| token.clone())
        .collect();
    for token in &tokens {
        let attempt = inner.attempts.get(token).expect("collected token");
        if matches!(attempt.terminal, Some(Terminal::Failure(_))) {
            continue;
        }
        let failure = ReadTextFailure::for_resource(
            ReadTextFailureKind::Unavailable,
            &attempt.pending.resource,
            attempt.pending.range,
        );
        terminalize_locked(
            state,
            &mut inner,
            token,
            Terminal::Failure(Box::new(failure)),
        );
    }
    tokens
}

pub(crate) fn drain_receipts(state: &ReadState, owner: &ReadTextOwner) -> Vec<TerminalReceipt> {
    let mut inner = lock(state);
    let receipts = std::mem::take(&mut inner.receipts);
    let (mine, other): (Vec<_>, Vec<_>) = receipts
        .into_iter()
        .partition(|receipt| &receipt.owner == owner);
    inner.receipts = other;
    debug_assert!(
        inner
            .attempts
            .len()
            .checked_add(inner.receipts.len())
            .is_some_and(|size| size <= MAX_PENDING_READS)
    );
    state.changed.notify_all();
    mine
}

#[allow(dead_code, reason = "P-02 Decision B receiver handoff")]
pub(crate) fn claim_receiver(
    state: &Arc<ReadState>,
    token: &str,
) -> ReadTextResult<ReadTextCompletionLease> {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    };
    match attempt.terminal.take() {
        Some(Terminal::Success(completion)) => {
            attempt.phase = AttemptPhase::Leased;
            state.changed.notify_all();
            Ok(ReadTextCompletionLease::new(
                Arc::clone(state),
                token.into(),
                attempt.pending.deadline,
                completion,
            ))
        }
        Some(Terminal::Failure(failure)) => {
            remove_attempt_locked(state, &mut inner, token);
            Err(failure)
        }
        None => Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable)),
    }
}

#[allow(dead_code, reason = "P-02 Decision B receiver fallback")]
pub(crate) fn receiver_dropped(state: &ReadState, token: &str) {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return;
    };
    let payload = match attempt.terminal.take() {
        Some(Terminal::Failure(failure)) => failure.receipt,
        Some(Terminal::Success(_)) | None => {
            delivery_unknown(&attempt.pending.resource, attempt.pending.range)
        }
    };
    fallback_locked(state, &mut inner, token, payload);
}

#[allow(dead_code, reason = "P-02 Decision B lease fallback")]
pub(crate) fn lease_dropped(state: &ReadState, token: &str) {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return;
    };
    let payload = match attempt
        .pending_terminal
        .take()
        .or_else(|| attempt.terminal.take())
    {
        Some(Terminal::Failure(failure)) if attempt.phase != AttemptPhase::Delivering => {
            failure.receipt
        }
        _ => delivery_unknown(&attempt.pending.resource, attempt.pending.range),
    };
    fallback_locked(state, &mut inner, token, payload);
}

#[allow(dead_code, reason = "P-02 sealed egress transition")]
pub(crate) fn begin_delivery(state: &ReadState, token: &str) -> ReadTextResult<P02Cancellation> {
    let mut inner = lock(state);
    let attempt = inner
        .attempts
        .get_mut(token)
        .ok_or(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable))?;
    if attempt.phase == AttemptPhase::Revoked {
        let failure = match attempt.terminal.take() {
            Some(Terminal::Failure(failure)) => failure,
            _ => ReadTextFailure::terminal(ReadTextFailureKind::Unavailable),
        };
        remove_attempt_locked(state, &mut inner, token);
        return Err(failure);
    }
    if attempt.phase != AttemptPhase::Leased {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    }
    attempt.phase = AttemptPhase::Delivering;
    let cancellation = attempt.cancellation.clone();
    state.changed.notify_all();
    Ok(cancellation)
}

#[allow(dead_code, reason = "P-02 sealed egress settlement")]
pub(crate) fn settle_lease(
    state: &ReadState,
    token: &str,
    outcome: P02EgressOutcome,
    completion: ReadTextCompletion,
) -> ReadTextResult<ReadTextDelivery> {
    let mut inner = lock(state);
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    };
    if attempt.phase != AttemptPhase::Delivering {
        return Err(ReadTextFailure::terminal(ReadTextFailureKind::Unavailable));
    }
    let lifecycle = attempt.pending_terminal.take();
    let deadline_elapsed = Instant::now() >= attempt.pending.deadline;
    let (delivery, payload) = match (outcome, lifecycle, deadline_elapsed) {
        (P02EgressOutcome::ZeroByteFailure, Some(Terminal::Failure(failure)), _) => {
            (DeliveryKind::Zero, failure.receipt)
        }
        (P02EgressOutcome::ZeroByteFailure, _, _) => (
            DeliveryKind::Zero,
            receipt(
                Some(&attempt.pending.resource),
                attempt.pending.range,
                "native",
                "zeroByteFailure",
                0,
            ),
        ),
        (P02EgressOutcome::Complete, None, false) => (DeliveryKind::Complete, completion.receipt),
        (P02EgressOutcome::Complete, _, _) | (P02EgressOutcome::EffectUnknown { .. }, _, _) => (
            DeliveryKind::Unknown,
            delivery_unknown(&attempt.pending.resource, attempt.pending.range),
        ),
    };
    let terminal = TerminalReceipt {
        owner: attempt.pending.owner.clone(),
        token: token.into(),
        resource: attempt.pending.resource.clone(),
        range: attempt.pending.range,
        payload,
    };
    remove_attempt_locked(state, &mut inner, token);
    Ok(match delivery {
        DeliveryKind::Complete => ReadTextDelivery::Complete(terminal),
        DeliveryKind::Zero => ReadTextDelivery::ZeroByteFailure(terminal),
        DeliveryKind::Unknown => ReadTextDelivery::EffectUnknown(terminal),
    })
}
#[allow(dead_code, reason = "P-02 sealed egress settlement")]
enum DeliveryKind {
    Complete,
    Zero,
    Unknown,
}

pub(super) fn terminalize_locked(
    state: &ReadState,
    inner: &mut ReadStateInner,
    token: &str,
    terminal: Terminal,
) {
    let Some(attempt) = inner.attempts.get_mut(token) else {
        return;
    };
    if attempt.phase == AttemptPhase::Delivering {
        if attempt.pending_terminal.is_none() {
            attempt.pending_terminal = Some(terminal);
            attempt.cancellation.cancel();
        }
        state.changed.notify_all();
        return;
    }
    if attempt.phase == AttemptPhase::Leased {
        attempt.phase = AttemptPhase::Revoked;
        attempt.terminal = Some(terminal);
        release_reservation_locked(inner, token);
        state.changed.notify_all();
        return;
    }
    if matches!(attempt.terminal, Some(Terminal::Failure(_))) {
        return;
    }
    attempt.terminal = Some(terminal);
    attempt.phase = AttemptPhase::Revoked;
    let disconnected = attempt
        .notify
        .take()
        .is_none_or(|sender| sender.send(()).is_err());
    if disconnected {
        let attempt = inner.attempts.get_mut(token).expect("attempt exists");
        let payload = match attempt.terminal.take().expect("terminal exists") {
            Terminal::Failure(failure) => failure.receipt,
            Terminal::Success(_) => {
                delivery_unknown(&attempt.pending.resource, attempt.pending.range)
            }
        };
        fallback_locked(state, inner, token, payload);
    } else {
        state.changed.notify_all();
    }
}

fn fallback_locked(
    state: &ReadState,
    inner: &mut ReadStateInner,
    token: &str,
    payload: NormalizedPayload,
) {
    let attempt = inner.attempts.get(token).expect("attempt exists");
    let receipt = TerminalReceipt {
        owner: attempt.pending.owner.clone(),
        token: token.into(),
        resource: attempt.pending.resource.clone(),
        range: attempt.pending.range,
        payload,
    };
    assert!(
        inner.receipts.len() < MAX_PENDING_READS,
        "attempt-to-receipt transfer preserves bounded occupancy"
    );
    inner.receipts.push(receipt);
    remove_attempt_locked(state, inner, token);
}
fn remove_attempt_locked(state: &ReadState, inner: &mut ReadStateInner, token: &str) {
    if let Some(attempt) = inner.attempts.remove(token) {
        release_owner_token_locked(inner, &attempt.pending.owner, token);
    }
    debug_assert!(
        inner
            .attempts
            .len()
            .checked_add(inner.receipts.len())
            .is_some_and(|size| size <= MAX_PENDING_READS)
    );
    state.changed.notify_all();
}
fn release_reservation_locked(inner: &mut ReadStateInner, token: &str) {
    if let Some(owner) = inner
        .attempts
        .get(token)
        .map(|attempt| attempt.pending.owner.clone())
    {
        release_owner_token_locked(inner, &owner, token);
    }
}
fn release_owner_token_locked(inner: &mut ReadStateInner, owner: &ReadTextOwner, token: &str) {
    let key = owner_session_key(owner);
    if inner
        .reservations
        .get(&key)
        .is_some_and(|reserved| reserved == token)
    {
        inner.reservations.remove(&key);
    }
}
pub(crate) fn lock(state: &ReadState) -> std::sync::MutexGuard<'_, ReadStateInner> {
    state
        .inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
