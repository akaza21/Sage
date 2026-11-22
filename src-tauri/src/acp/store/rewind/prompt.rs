//! Only send-time receipts identify transmitted prompts. Display data cannot.
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

use crate::acp::store::StoreError;

pub(super) fn digest(
    connection: &Connection,
    session: &str,
    turn: &str,
) -> Result<Option<[u8; 32]>, StoreError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct SendIntent {
        native_prompt_digest: Option<[u8; 32]>,
    }
    let saved: Option<Option<String>> = connection
        .query_row(
            "SELECT CASE WHEN payload_bytes<=4096 THEN payload_json END FROM session_facts
         WHERE session_id=?1 AND turn_id=?2 AND kind='sendIntent' LIMIT 1",
            params![session, turn],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(saved) = saved {
        let saved: SendIntent =
            serde_json::from_str(saved.as_deref().ok_or(StoreError::PayloadTooLarge)?)
                .map_err(|_| StoreError::Corrupt("native prompt digest"))?;
        return Ok(saved.native_prompt_digest);
    }
    Ok(None)
}
