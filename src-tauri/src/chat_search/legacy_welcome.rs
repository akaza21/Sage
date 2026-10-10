use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

use super::{ChatRow, map_deadline};

pub(super) fn excluded_turns(
    connection: &rusqlite::Connection,
    chats: &[ChatRow],
    started: Instant,
    deadline: Duration,
) -> AppResult<String> {
    let mut welcome_turns = Vec::new();
    for chat in chats {
        if let Some(turn) = map_deadline(
            crate::session_events::legacy_welcome::turn_id(connection, &chat.id),
            started,
            deadline,
        )? {
            welcome_turns.push(turn);
        }
    }
    serde_json::to_string(&welcome_turns).map_err(|error| {
        AppError::InvalidRequest(format!("Chat search exclusion encoding failed: {error}"))
    })
}
