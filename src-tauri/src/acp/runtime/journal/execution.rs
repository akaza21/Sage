use super::*;

pub(super) fn execute_envelope<S: AcpStore>(
    queue: &Arc<Queue>,
    store: &mut S,
    envelope: Envelope,
    prepare: &Arc<workspace_capture::Prepare>,
) {
    if now_millis() >= envelope.deadline_millis {
        workspace_capture::discard(&envelope);
        envelope
            .acknowledgement
            .complete(Err(JournalError::Deadline));
        return;
    }
    let mut envelope = match workspace_capture::defer(queue, envelope, prepare) {
        Some(envelope) => envelope,
        None => return,
    };
    let execute = {
        let state = lock(queue);
        !state.aborted && envelope.acknowledgement.begin_execution()
    };
    if !execute {
        envelope.acknowledgement.close();
        workspace_capture::discard(&envelope);
        return;
    }
    let result = envelope
        .workspace_result
        .take()
        .unwrap_or(Ok(()))
        .and_then(|()| store.commit(&envelope.mutations));
    let store_commit_unknown = matches!(&result, Err(StoreError::CommitUnknown));
    crate::turn_workspace::discard_rejected_staging(
        envelope.workspace_database.as_ref(),
        &envelope.mutations,
        &result,
    );
    let caller_commit_unknown = envelope
        .acknowledgement
        .complete(result.map_err(JournalError::Store));
    if (store_commit_unknown || caller_commit_unknown)
        && let Some(reservation) = envelope.reservation.take()
    {
        lock(queue).unknown_reservations.push(reservation);
    }
}
