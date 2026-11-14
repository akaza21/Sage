use rusqlite::{OptionalExtension, Transaction, params};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;

use sage_core::limits::LIMITS;

use super::super::{
    MAX_GLOBAL_EVENT_BYTES, MAX_GLOBAL_EVENTS, MAX_SESSION_EVENT_BYTES, MAX_SESSION_EVENTS,
};

const MAX_SESSION_FACTS: i64 = 20_000;
const MAX_GLOBAL_FACTS: i64 = 200_000;
const MAX_FACT_BYTES: i64 = 67_108_864;
const MAX_SESSION_PAYLOAD_BYTES: i64 = 268_435_456;
const MAX_SHARED_PAYLOAD_BYTES: i64 = 536_870_912;

pub(super) struct Usage {
    pub(super) session_turns: i64,
    pub(super) session_items: i64,
    session_item_bytes: i64,
    pub(super) session_facts: i64,
    session_fact_bytes: i64,
    pub(super) session_payload_bytes: i64,
    pub(super) project_turns: i64,
    pub(super) store_turns: i64,
    pub(super) store_items: i64,
    store_item_bytes: i64,
    store_facts: i64,
    store_fact_bytes: i64,
    pub(super) store_payload_bytes: i64,
}

pub(super) fn load(
    transaction: &Transaction<'_>,
    project_id: &str,
    session_id: &str,
) -> AppResult<Usage> {
    transaction
        .query_row(
            "SELECT u.turn_count,u.item_count,u.item_bytes,u.fact_count,u.fact_bytes,u.payload_bytes,
                    p.turn_count,g.turn_count,g.item_count,g.item_bytes,g.fact_count,g.fact_bytes,
                    g.shared_payload_bytes
             FROM acp_session_usage u
             JOIN acp_project_usage p ON p.project_id=u.project_id
             JOIN acp_store_usage g ON g.singleton=1
             WHERE u.session_id=?1 AND u.project_id=?2",
            params![session_id, project_id],
            |row| {
                Ok(Usage {
                    session_turns: row.get(0)?,
                    session_items: row.get(1)?,
                    session_item_bytes: row.get(2)?,
                    session_facts: row.get(3)?,
                    session_fact_bytes: row.get(4)?,
                    session_payload_bytes: row.get(5)?,
                    project_turns: row.get(6)?,
                    store_turns: row.get(7)?,
                    store_items: row.get(8)?,
                    store_item_bytes: row.get(9)?,
                    store_facts: row.get(10)?,
                    store_fact_bytes: row.get(11)?,
                    store_payload_bytes: row.get(12)?,
                })
            },
        )
        .optional().db()?
        .ok_or_else(|| AppError::InvalidRequest("simulator usage ledger is absent".into()))
}

pub(super) fn ensure(usage: &Usage, item_bytes: i64, fact_bytes: i64) -> AppResult<()> {
    let session_over = usage.session_turns + 1 > LIMITS.durable.turns_per_session as i64
        || usage.session_items + 2 > MAX_SESSION_EVENTS
        || usage.session_item_bytes.saturating_add(item_bytes) > MAX_SESSION_EVENT_BYTES
        || usage.session_facts + 1 > MAX_SESSION_FACTS
        || usage.session_fact_bytes.saturating_add(fact_bytes) > MAX_FACT_BYTES
        || usage
            .session_payload_bytes
            .saturating_add(item_bytes)
            .saturating_add(fact_bytes)
            > MAX_SESSION_PAYLOAD_BYTES;
    let global_over = usage.project_turns + 1 > LIMITS.durable.turns_per_project as i64
        || usage.store_turns + 1 > LIMITS.durable.turns_global as i64
        || usage.store_items + 2 > MAX_GLOBAL_EVENTS
        || usage.store_item_bytes.saturating_add(item_bytes) > MAX_GLOBAL_EVENT_BYTES
        || usage.store_facts + 1 > MAX_GLOBAL_FACTS
        || usage.store_fact_bytes.saturating_add(fact_bytes) > MAX_FACT_BYTES
        || usage
            .store_payload_bytes
            .saturating_add(item_bytes)
            .saturating_add(fact_bytes)
            > MAX_SHARED_PAYLOAD_BYTES;
    if session_over || global_over {
        return Err(AppError::ResourceLimit(
            "simulator transcript capacity reached; existing history was preserved".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(session_turns: i64, session_items: i64) -> Usage {
        Usage {
            session_turns,
            session_items,
            session_item_bytes: 0,
            session_facts: 0,
            session_fact_bytes: 0,
            session_payload_bytes: 0,
            project_turns: session_turns,
            store_turns: session_turns,
            store_items: session_items,
            store_item_bytes: 0,
            store_facts: 0,
            store_fact_bytes: 0,
            store_payload_bytes: 0,
        }
    }

    #[test]
    fn ade_0005_item_boundary_is_not_masked_by_a_smaller_turn_limit() {
        assert_eq!(LIMITS.durable.turns_per_session, MAX_SESSION_EVENTS as u64);
        assert!(ensure(&usage(1_024, 2_047), 2, 1).is_ok());
        assert!(ensure(&usage(2_499, 4_999), 2, 1).is_err());
    }
}
