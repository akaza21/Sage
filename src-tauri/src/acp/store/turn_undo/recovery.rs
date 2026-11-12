//! Reconcile undo plans only across a proven zero-process boundary.
use super::*;

/// Reconcile the exact owner's plan inside runtime recovery's transaction.
/// Native-only uncertainty rolls back while that session is still mutable;
/// the same transaction retires it before any send can pass the intent gate.
pub(crate) fn settle_live(
    tx: &Transaction<'_>,
    candidate: &super::super::RecoveryCandidate,
    now: i64,
) -> Result<(), StoreError> {
    let Some(mut intent) = intent::pending(
        tx,
        candidate.project_id.as_str(),
        candidate.chat_id.as_str(),
    )?
    else {
        return Ok(());
    };
    if intent.command.session_id != candidate.session_id
        || intent.command.runtime_epoch != candidate.runtime_epoch
    {
        return Err(StoreError::Corrupt("undo recovery owner"));
    }
    intent.command.now_millis = now;
    if intent.command.harness_reverted {
        intent::rollback_in(tx, &intent.command)
    } else {
        super::record_turn_undone_in(tx, &intent.command).map(|_| ())
    }
}

/// Shutdown may park a runtime between its native reply and settlement.
/// Stream cold intents too; the live-epoch recovery scan cannot see them.
pub(crate) fn recover_parked(
    connection: &mut Connection,
    now: i64,
) -> Result<(usize, Option<ProjectionRevision>), StoreError> {
    let (mut recovered, mut latest) = (0, None);
    loop {
        let row: Option<(String, String)> = connection
            .query_row(
                "SELECT f.project_id,f.chat_id FROM session_facts f
             WHERE f.kind='turnUndoStarted'
               AND NOT EXISTS(SELECT 1 FROM session_facts s WHERE s.session_id=f.session_id
                 AND s.turn_id=f.turn_id AND s.kind='turnUndoSettled'
                 AND json_extract(s.payload_json,'$.start')=f.sequence)
               AND NOT EXISTS(SELECT 1 FROM session_runtime_epochs e WHERE e.session_id=f.session_id
                 AND e.state IN ('starting','running','terminating'))
             ORDER BY f.created_at,f.session_id,f.sequence LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((project, chat)) = row else {
            return Ok((recovered, latest));
        };
        let tx = connection.transaction()?;
        let mut intent = intent::pending(&tx, &project, &chat)?
            .ok_or(StoreError::Corrupt("parked undo intent"))?;
        let command = &mut intent.command;
        command.now_millis = now;
        let (state, residency): (String, String) = tx.query_row(
            "SELECT s.state,s.residency FROM agent_sessions s
             JOIN session_runtime_epochs e ON e.session_id=s.id AND e.runtime_epoch=?2
             WHERE s.id=?1 AND e.state IN ('exited','failed','aborted')",
            params![
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let revision = if state == "ready" && residency == "cold" && !command.harness_reverted {
            super::record_turn_undone_in(&tx, command)?
                .published
                .ok_or(StoreError::Corrupt("parked undo publication"))?
        } else {
            // Uncertain native-only context is never resumed. A terminal
            // session already supplies that boundary and stays immutable.
            let terminal = matches!(state.as_str(), "failed" | "closed" | "incompatible");
            if !terminal && (state != "ready" || residency != "cold") {
                return Err(StoreError::Corrupt("parked undo owner"));
            }
            intent::release_reservation(&tx, command)?;
            let revision = next_projection_revision(&tx, &project, &chat)?;
            intent::append_settled(&tx, &intent, "rolledBack", revision, now)?;
            if !terminal {
                tx.execute(
                    "DELETE FROM chat_session_heads WHERE session_id=?1",
                    [intent.command.session_id.as_str()],
                )?;
                tx.execute(
                    "UPDATE agent_sessions SET state='failed',revision=revision+1,
                    projection_revision=?2,updated_at=?3,terminal_at=?3 WHERE id=?1",
                    params![
                        intent.command.session_id.as_str(),
                        revision.get() as i64,
                        now
                    ],
                )?;
            }
            tx.execute("UPDATE chats SET projection_revision=?3,updated_at=?4 WHERE project_id=?1 AND id=?2",
                params![project, chat, revision.get() as i64, now])?;
            revision
        };
        tx.commit().map_err(|_| StoreError::CommitUnknown)?;
        recovered += 1;
        latest = Some(revision);
    }
}
