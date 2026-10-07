use rusqlite::{OptionalExtension, ToSql, Transaction, params_from_iter};

use crate::acp::{
    contract::EntityRevision,
    store::{StoreError, next_projection_revision},
};

use super::{AdapterStoreOwner, TurnUsageSnapshotCommand};

const SELECT: &str = "SELECT input_tokens,output_tokens,context_window,revision
     FROM acp_turn_usage_snapshots
     WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5";

const UPDATE: &str = "UPDATE acp_turn_usage_snapshots
     SET input_tokens=?6,output_tokens=?7,context_window=?8,updated_at=?9,revision=?10
     WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4 AND runtime_epoch=?5";

const INSERT: &str = "INSERT INTO acp_turn_usage_snapshots
     (project_id,chat_id,session_id,turn_id,runtime_epoch,input_tokens,output_tokens,
      context_window,revision,created_at,updated_at)
     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,?9,?9)";

struct OwnerKey<'a> {
    project_id: &'a str,
    chat_id: &'a str,
    session_id: &'a str,
    turn_id: &'a str,
    runtime_epoch: i64,
}

impl OwnerKey<'_> {
    fn arguments(&self) -> [&dyn ToSql; 5] {
        [
            &self.project_id,
            &self.chat_id,
            &self.session_id,
            &self.turn_id,
            &self.runtime_epoch,
        ]
    }
}

pub(super) fn apply(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &TurnUsageSnapshotCommand,
) -> Result<bool, StoreError> {
    let input_tokens =
        i64::try_from(command.input_tokens).map_err(|_| StoreError::CounterExhausted)?;
    let output_tokens =
        i64::try_from(command.output_tokens).map_err(|_| StoreError::CounterExhausted)?;
    let runtime_epoch =
        i64::try_from(owner.runtime_epoch.get()).map_err(|_| StoreError::CounterExhausted)?;
    let key = OwnerKey {
        project_id: owner.project_id.as_str(),
        chat_id: owner.chat_id.as_str(),
        session_id: owner.session_id.as_str(),
        turn_id: owner.turn_id.as_str(),
        runtime_epoch,
    };
    let current: Option<(i64, i64, Option<i64>, i64)> = transaction
        .query_row(SELECT, params_from_iter(key.arguments()), |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .optional()?;
    let context_window = command.context_window.map(i64::from);
    let revision = match current {
        Some((current_input, current_output, current_window, _))
            if current_input == input_tokens
                && current_output == output_tokens
                && current_window == context_window =>
        {
            return Ok(false);
        }
        Some((_, _, _, revision)) => Some(next_revision(revision)?),
        None => None,
    };
    let projection = next_projection_revision(
        transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
    )?;
    let mut arguments = key.arguments().to_vec();
    arguments.extend([
        &input_tokens as &dyn ToSql,
        &output_tokens,
        &context_window,
        &command.now_millis,
    ]);
    match revision {
        Some(ref revision) => {
            arguments.push(revision);
            transaction.execute(UPDATE, params_from_iter(arguments))?;
        }
        None => {
            transaction.execute(INSERT, params_from_iter(arguments))?;
        }
    }
    super::super::facts::publish_projection(
        transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
        projection,
        command.now_millis,
    )?;
    Ok(true)
}

fn next_revision(revision: i64) -> Result<i64, StoreError> {
    let revision = EntityRevision::new(
        u64::try_from(revision).map_err(|_| StoreError::Corrupt("usage revision"))?,
    )
    .map_err(|_| StoreError::Corrupt("usage revision"))?
    .next()
    .map_err(|_| StoreError::CounterExhausted)?;
    i64::try_from(revision.get()).map_err(|_| StoreError::CounterExhausted)
}
