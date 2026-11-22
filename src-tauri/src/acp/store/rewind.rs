//! Durable native turn correlations and admission of conversation targets.
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use sage_acp_protocol::{intent::ExternalId, session_control::ConversationTarget};

use super::{AdapterStoreOwner, StoreError, facts};

mod prompt;

pub(crate) fn record_native_turn(
    tx: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    native: &ExternalId,
    now: i64,
) -> Result<(), StoreError> {
    let existing: Option<String> = tx
        .query_row(
            "SELECT json_extract(payload_json,'$.nativeTurnId') FROM session_facts
         WHERE session_id=?1 AND turn_id=?2 AND kind='nativeTurnTarget' LIMIT 1",
            params![owner.session_id.as_str(), owner.turn_id.as_str()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        return if existing == native.as_str() {
            Ok(())
        } else {
            Err(StoreError::Corrupt("native turn target"))
        };
    }
    let revision =
        facts::next_projection_revision(tx, owner.project_id.as_str(), owner.chat_id.as_str())?;
    facts::append_fact_payload(
        tx,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        owner.session_id.as_str(),
        Some(owner.turn_id.as_str()),
        "nativeTurnTarget",
        &serde_json::json!({"nativeTurnId":native}).to_string(),
        revision,
        now,
    )?;
    Ok(())
}

/// Both operations require a finished, visible prompted turn of the idle head.
/// A compacted prefix is never admitted even if the caller retained an old page.
pub(crate) fn target(
    connection: &Connection,
    project: &str,
    chat: &str,
    session: &str,
    turn: &str,
) -> Result<ConversationTarget, StoreError> {
    let ordinal: Option<i64> = connection.query_row(concat!(
        "SELECT t.ordinal FROM turns t JOIN chat_session_heads h ON h.session_id=t.session_id
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
         AND t.terminal_at IS NOT NULL AND t.delivery_state='correlated' AND ",
        super::turn_undo::visible_turn!("t.session_id","t.id")," AND ",super::prompted_turn!("t.session_id","t.id"),
        " AND NOT EXISTS(SELECT 1 FROM turns busy WHERE busy.session_id=?3 AND busy.terminal_at IS NULL)
         AND NOT EXISTS(SELECT 1 FROM items compact JOIN turns ct ON ct.id=compact.turn_id
           WHERE ct.session_id=?3 AND ct.ordinal>=t.ordinal AND compact.kind='compactionNotice')
         AND NOT EXISTS(SELECT 1 FROM session_facts f JOIN turns ct ON ct.id=f.turn_id
           WHERE f.session_id=?3 AND ct.ordinal>=t.ordinal AND f.kind='sessionActivity'
             AND json_extract(f.payload_json,'$.k')='compaction' AND json_extract(f.payload_json,'$.s')='completed')"
    ),params![project,chat,session,turn],|r|r.get(0)).optional()?;
    let ordinal = ordinal.ok_or(StoreError::Conflict {
        resource: "rewindTarget",
    })?;
    let native: Option<String> = connection
        .query_row(
            "SELECT json_extract(payload_json,'$.nativeTurnId') FROM session_facts
         WHERE session_id=?1 AND turn_id=?2 AND kind='nativeTurnTarget' LIMIT 1",
            params![session, turn],
            |r| r.get(0),
        )
        .optional()?;
    let prompt_digest = if native.is_none() {
        prompt::digest(connection, session, turn)?
    } else {
        None
    };
    if native.is_none() && prompt_digest.is_none() {
        return Err(StoreError::RewindUnavailable {
            reason: "Older messages can't be rewound.",
        });
    }
    // Native receipts stand alone, including in legacy history. Only a
    // send-time digest needs a position among proven delivered prompts.
    let following: i64 = if native.is_some() {
        0
    } else {
        connection.query_row(concat!(
        "SELECT COUNT(*) FROM items i JOIN turns t ON t.id=i.turn_id WHERE t.session_id=?1
         AND t.delivery_state='correlated' AND t.ordinal>=?2 AND i.kind='text' AND (json_extract(i.payload_json,'$.kind')='userPrompt' OR (json_extract(i.payload_json,'$.payload.role')='user'
         AND t.ordinal>?2)) AND ",super::turn_undo::visible_turn!("t.session_id","t.id")
    ),params![session,ordinal],|r|r.get(0))?
    };
    Ok(ConversationTarget {
        turn_id: sage_acp_protocol::contract::SageTurnId::parse(turn)
            .map_err(|_| StoreError::Corrupt("rewind turn"))?,
        native_turn_id: native
            .map(|id| {
                ExternalId::new("nativeTurnId", id).map_err(|_| StoreError::Corrupt("native turn"))
            })
            .transpose()?,
        following_messages: u32::try_from(following)
            .map_err(|_| StoreError::Corrupt("rewind position"))?,
        prompt_digest,
    })
}

/// Page decoration uses the same durable identity rule as target admission.
/// Delivery must be proven even when a queued prompt already has a digest.
pub(crate) fn has_identity(
    connection: &Connection,
    session: &str,
    turn: &str,
) -> Result<bool, StoreError> {
    let identity: Option<(bool, bool)> = connection
        .query_row(
            "SELECT t.delivery_state='correlated', EXISTS(SELECT 1 FROM session_facts f
           WHERE f.session_id=t.session_id AND f.turn_id=t.id AND f.kind='nativeTurnTarget'
             AND json_extract(f.payload_json,'$.nativeTurnId') IS NOT NULL)
         FROM turns t WHERE t.session_id=?1 AND t.id=?2",
            params![session, turn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((delivered, native)) = identity else {
        return Ok(false);
    };
    Ok(delivered && (native || prompt::digest(connection, session, turn)?.is_some()))
}

pub(crate) fn suffix(
    connection: &Connection,
    project: &str,
    chat: &str,
    session: &str,
    turn: &str,
) -> Result<Vec<sage_acp_protocol::contract::SageTurnId>, StoreError> {
    // Validate the boundary before reserving any effect.
    target(connection, project, chat, session, turn)?;
    let mut statement = connection.prepare(concat!(
        "SELECT t.id FROM turns t WHERE t.session_id=?1 AND t.ordinal >=
         (SELECT ordinal FROM turns WHERE session_id=?1 AND id=?2) AND ",
        super::turn_undo::visible_turn!("t.session_id", "t.id"),
        " ORDER BY t.ordinal DESC LIMIT 101"
    ))?;
    let rows = statement.query_map(params![session, turn], |r| r.get::<_, String>(0))?;
    let mut turns = Vec::new();
    for row in rows {
        if turns.len() == 100 {
            return Err(StoreError::PayloadTooLarge);
        }
        turns.push(
            sage_acp_protocol::contract::SageTurnId::parse(row?)
                .map_err(|_| StoreError::Corrupt("rewind suffix"))?,
        );
    }
    Ok(turns)
}
