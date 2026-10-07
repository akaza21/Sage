use std::sync::Weak;

use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, RuntimeHost, TerminalNotification},
    store::AdapterTurnCursor,
};

use super::WorkerState;
use crate::acp::runtime::process_driver::{
    FirstTurnWorkerState, StdinTransport, WorkerCommand, WorkerEnvelope,
};

pub(super) struct PendingTerminal {
    cursor: AdapterTurnCursor,
    notification: TerminalNotification,
}

pub(super) fn queue<A, R, T, C>(
    state: &WorkerState<A, R, T, C>,
    host: &Weak<RuntimeHost>,
    notified: &Option<AdapterTurnCursor>,
    pending: &mut Option<PendingTerminal>,
) -> Result<(), HostError>
where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: crate::acp::runtime::ProcessTreeControl,
    C::ZeroProof: crate::acp::runtime::process_driver::CoordinatorZeroToken,
{
    if pending.is_some() {
        return Ok(());
    }
    let FirstTurnWorkerState::Active(active) = state else {
        return Ok(());
    };
    let Some(cursor) = active
        .turn
        .driver
        .latest_durable_turn_cursor()
        .filter(|cursor| cursor.state().is_terminal())
        .cloned()
    else {
        return Ok(());
    };
    if notified.as_ref() == Some(&cursor) {
        return Ok(());
    }
    let notification = host
        .upgrade()
        .ok_or(HostError::Closed)?
        .try_notify_terminal(active.turn.driver.runtime_owner().clone(), cursor.clone())?;
    *pending = Some(PendingTerminal {
        cursor,
        notification,
    });
    Ok(())
}

pub(super) fn cursor(envelope: &WorkerEnvelope) -> Option<AdapterTurnCursor> {
    match &envelope.command {
        WorkerCommand::AcceptTerminal { cursor, .. } => Some(cursor.clone()),
        _ => None,
    }
}

pub(super) fn acknowledge(
    callback: &AdapterTurnCursor,
    pending: &mut Option<PendingTerminal>,
    notified: &mut Option<AdapterTurnCursor>,
) -> Result<(), HostError> {
    let notification = pending.take().ok_or(HostError::Intent)?;
    if &notification.cursor != callback {
        return Err(HostError::Intent);
    }
    notification.notification.wait()?;
    *notified = Some(notification.cursor);
    Ok(())
}
