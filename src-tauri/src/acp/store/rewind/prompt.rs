//! Recover a native prompt identity without confusing display text with input.
use rusqlite::{Connection, OptionalExtension, params};
use sage_acp_protocol::contract::SafeText;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::acp::{
    attached_context::{AttachedContexts, effective_prompt},
    store::StoreError,
    transcript_quote::Prompt,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    text: SafeText<262144>,
    #[serde(default)]
    attached_context: AttachedContexts,
    #[serde(default)]
    attachments: u8,
    session_action: Option<serde::de::IgnoredAny>,
    command: Option<serde::de::IgnoredAny>,
}

pub(super) fn digest(
    connection: &Connection,
    session: &str,
    turn: &str,
) -> Result<[u8; 32], StoreError> {
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
        if let Some(digest) = saved.native_prompt_digest {
            return Ok(digest);
        }
    }
    #[derive(Deserialize)]
    struct Item {
        payload: Identity,
    }
    let payload: Option<String> = connection.query_row(
        "SELECT CASE WHEN payload_bytes<=262144 THEN payload_json END FROM items
         WHERE session_id=?1 AND turn_id=?2 AND kind='text'
           AND json_extract(payload_json,'$.payload.role')='user' ORDER BY ordinal LIMIT 1",
        params![session, turn],
        |r| r.get(0),
    )?;
    let item: Item = serde_json::from_str(payload.as_deref().ok_or(StoreError::PayloadTooLarge)?)
        .map_err(|_| unavailable())?;
    let identity = item.payload;
    // Older file snapshots were deliberately never persisted. Command and
    // action text may also have been transformed by the native binding.
    if identity.attachments > 0 || identity.session_action.is_some() || identity.command.is_some() {
        return Err(StoreError::RewindUnavailable {
            reason: "This historical prompt has no saved native identity and its transmitted context cannot be rebuilt.",
        });
    }
    let original =
        Prompt::new("rewindPrompt", identity.text.as_str()).map_err(|_| unavailable())?;
    let effective =
        effective_prompt(&original, &identity.attached_context).map_err(|_| unavailable())?;
    Ok(Sha256::digest(effective.as_str().as_bytes()).into())
}

fn unavailable() -> StoreError {
    StoreError::RewindUnavailable {
        reason: "This historical prompt's transmitted context cannot be rebuilt; it is not a rewind target.",
    }
}
