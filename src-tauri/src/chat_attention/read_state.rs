use super::{
    contract::{ChatAttentionV1, Position},
    store,
};
use crate::acp::store::StoreError;
use rusqlite::{Connection, params};

pub(crate) fn ack(
    connection: &mut Connection,
    project: &str,
    chat: &str,
    observed: Position,
) -> Result<ChatAttentionV1, StoreError> {
    let transaction = connection.transaction()?;
    let summary = store::get_one(&transaction, project, chat)?;
    if observed > summary.activity_position {
        return Err(StoreError::Conflict {
            resource: "attentionPosition",
        });
    }
    if summary
        .read_position
        .is_none_or(|previous| observed > previous)
    {
        transaction.execute(
            "INSERT INTO chat_attention(chat_id) VALUES(?1) ON CONFLICT DO NOTHING",
            [chat],
        )?;
        transaction.execute(
            "UPDATE chat_attention SET read_position=?2,revision=?3 WHERE chat_id=?1",
            params![
                chat,
                observed.get() as i64,
                store::increment(summary.revision.get() as i64)?
            ],
        )?;
    }
    let result = store::get_one(&transaction, project, chat)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(result)
}

pub(crate) fn mark_unread(
    connection: &mut Connection,
    project: &str,
    chat: &str,
    expected: Position,
) -> Result<ChatAttentionV1, StoreError> {
    let transaction = connection.transaction()?;
    let summary = store::get_one(&transaction, project, chat)?;
    if expected != summary.revision {
        return Err(StoreError::Conflict {
            resource: "attentionRevision",
        });
    }
    let next = store::increment(summary.activity_position.get() as i64)?;
    transaction.execute(
        "INSERT INTO chat_attention(chat_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [chat],
    )?;
    transaction.execute("UPDATE chat_attention SET activity_position=?2,forced_unread_position=?2,revision=?3 WHERE chat_id=?1",
        params![chat,next,store::increment(summary.revision.get() as i64)?])?;
    let result = store::get_one(&transaction, project, chat)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(result)
}
