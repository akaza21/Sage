use super::*;

pub(super) fn commit_batch(
    connection: &mut rusqlite::Connection,
    batch: &[StoreMutation],
) -> Result<StoreCommit, StoreError> {
    if batch.is_empty()
        || batch.len()
            > crate::acp::limits::LIMITS
                .queues
                .journal_facts_per_transaction as usize
    {
        return Err(StoreError::BatchShape);
    }
    validate_batch_shape(batch)?;
    if batch[0].is_standalone() {
        return mutations::standalone_commit(connection, &batch[0]);
    }
    let transaction = connection.transaction()?;
    let mut results = Vec::with_capacity(batch.len());
    let mut projection_revision = ProjectionRevision::ZERO;
    for mutation in batch {
        let result = match mutation {
            StoreMutation::LookupTurn(command) => {
                let prepared = lookup_prepared_turn(&transaction, command)?;
                projection_revision = prepared
                    .as_ref()
                    .map_or(command.expected_projection_revision, |value| {
                        value.projection_revision
                    });
                StoreMutationResult::Lookup(prepared)
            }
            StoreMutation::LookupActiveTurn(command) => {
                let turn = lookup_active_turn(&transaction, command)?;
                projection_revision = turn
                    .as_ref()
                    .map_or(ProjectionRevision::ZERO, |value| value.projection_revision);
                StoreMutationResult::ActiveTurnLookup(turn)
            }
            StoreMutation::PrepareTurn(command) => {
                let prepared = commands::prepare_turn_in(&transaction, command.clone())?;
                projection_revision = prepared.projection_revision;
                StoreMutationResult::Prepared(prepared)
            }
            StoreMutation::BeginSend(command) => {
                let turn = send::begin_send_in(&transaction, command.clone())?;
                projection_revision = turn.projection_revision;
                StoreMutationResult::Turn(turn)
            }
            StoreMutation::ObserveSend {
                command,
                observation,
                terminal_reason,
            } => {
                let turn = send::record_send_observation_in(
                    &transaction,
                    command.clone(),
                    *observation,
                    terminal_reason.clone(),
                )?;
                projection_revision = turn.projection_revision;
                StoreMutationResult::Turn(turn)
            }
            _ => unreachable!("standalone mutation dispatched before legacy transaction"),
        };
        results.push(result);
    }
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(StoreCommit {
        projection_revision,
        results,
    })
}

fn validate_batch_shape(batch: &[StoreMutation]) -> Result<(), StoreError> {
    if batch.iter().any(StoreMutation::is_standalone) {
        return if batch.len() == 1 {
            Ok(())
        } else {
            Err(StoreError::BatchShape)
        };
    }
    let first = mutation_shape(&batch[0]);
    let contains_lookup = first.2;
    for mutation in &batch[1..] {
        let shape = mutation_shape(mutation);
        if shape.0 != first.0
            || shape.1 != first.1
            || shape.2 != contains_lookup
            || (contains_lookup && !shape.2)
        {
            return Err(StoreError::BatchShape);
        }
    }
    Ok(())
}

fn mutation_shape(mutation: &StoreMutation) -> (&str, &str, bool) {
    match mutation {
        StoreMutation::LookupTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), true)
        }
        StoreMutation::LookupActiveTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), true)
        }
        StoreMutation::PrepareTurn(command) => {
            (command.project_id.as_str(), command.chat_id.as_str(), false)
        }
        StoreMutation::BeginSend(command) | StoreMutation::ObserveSend { command, .. } => {
            (command.project_id.as_str(), command.chat_id.as_str(), false)
        }
        _ => unreachable!("standalone mutations are rejected before legacy shape inspection"),
    }
}
