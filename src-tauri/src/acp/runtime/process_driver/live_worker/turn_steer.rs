use crate::acp::{
    bindings::adapter::{HarnessAdapter, ResourceMapper},
    runtime::{HostError, HostSteerIntent, SqliteAdapterBatchJournal, SteerCompletionSink},
};

use super::super::worker_command::WorkerResponder;
use super::super::{
    CoordinatorZeroToken, DriverError, PreparedProcessWrite, ProcessTreeControl, StdinTransport,
    WorkerReply,
};

pub(super) fn dispatch<A, R, T, C>(
    active: &mut PreparedProcessWrite<A, SqliteAdapterBatchJournal, R, T, C>,
    intent: HostSteerIntent,
    deadline: std::time::Instant,
    completion: SteerCompletionSink,
    responder: WorkerResponder,
) where
    A: HarnessAdapter,
    R: ResourceMapper,
    T: StdinTransport,
    C: ProcessTreeControl,
    C::ZeroProof: CoordinatorZeroToken,
{
    if intent.owner != *active.turn.driver.runtime_owner()
        || intent.turn_id != *active.turn.dispatch.turn_id()
        || active.ready_for_write()
        || active.has_durable_terminal()
        || active.cancel_intent().is_some()
        || active.durable_turn_state() == sage_acp_protocol::lifecycle::TurnState::Cancelling
        || active.steer_pending()
    {
        responder.respond(Err(HostError::Intent));
        completion.complete(Err(HostError::Intent));
        return;
    }
    let mut result = active
        .turn
        .driver
        .dispatch_steer(intent.prompt.clone(), intent.client_message_id.clone());
    if result
        .as_ref()
        .is_ok_and(|(progress, _)| progress.written_frames == 0 && progress.zero_byte_failures == 1)
    {
        result = active
            .turn
            .driver
            .dispatch_steer(intent.prompt, intent.client_message_id);
    }
    match result {
        Ok((progress, request_id))
            if progress.written_frames == 1 && progress.zero_byte_failures == 0 =>
        {
            active.begin_steer(request_id, deadline, completion);
            responder.respond(Ok(WorkerReply::Unit));
        }
        Err(DriverError::RejectedIntent) => {
            responder.respond(Err(HostError::Intent));
            completion.complete(Err(HostError::Intent));
        }
        _ => {
            responder.respond(Err(HostError::RecoveryRequired));
            completion.complete(Err(HostError::RecoveryRequired));
        }
    }
}
