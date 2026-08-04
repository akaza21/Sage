use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{SageChatId, SageProjectId},
    intent::PageCursor,
};

use super::{StoreError, history::TurnRow};

const DOMAIN: &[u8] = b"sage-history-cursor-v2";

#[derive(Debug, Clone)]
pub(super) struct Boundary {
    pub(super) session_ordinal: i64,
    pub(super) turn_ordinal: i64,
    pub(super) turn_id: String,
    pub(super) item_before: Option<i64>,
}

impl Boundary {
    pub(super) fn includes_boundary_turn(&self) -> bool {
        self.item_before.is_some()
    }
}

pub(super) fn resolve(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    cursor: PageCursor,
) -> Result<Boundary, StoreError> {
    let parts: Vec<_> = cursor.as_str().split(':').collect();
    let (kind, session_ordinal, turn_ordinal, item_before, digest) = match parts.as_slice() {
        ["v2", "t", session, turn, digest] => {
            ("t", counter(session)?, counter(turn)?, None, *digest)
        }
        ["v2", "i", session, turn, item, digest] => (
            "i",
            counter(session)?,
            counter(turn)?,
            Some(counter(item)?),
            *digest,
        ),
        _ => {
            return Err(StoreError::Conflict {
                resource: "pageCursor",
            });
        }
    };
    if digest.len() != 64
        || digest
            .bytes()
            .any(|byte| !byte.is_ascii_hexdigit() || byte.is_ascii_uppercase())
    {
        return Err(StoreError::Conflict {
            resource: "pageCursor",
        });
    }
    let turn_id = connection
        .query_row(
            "SELECT t.id FROM turns t
             JOIN agent_sessions s ON s.id=t.session_id
             WHERE t.project_id=?1 AND t.chat_id=?2
               AND s.ordinal=?3 AND t.ordinal=?4
             ORDER BY t.id LIMIT 2",
            params![
                project.as_str(),
                chat.as_str(),
                session_ordinal,
                turn_ordinal
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "pageCursor",
        })?;
    let expected = digest_for(
        project,
        chat,
        kind,
        session_ordinal,
        turn_ordinal,
        item_before,
        &turn_id,
    );
    if expected != digest {
        return Err(StoreError::Conflict {
            resource: "pageCursor",
        });
    }
    Ok(Boundary {
        session_ordinal,
        turn_ordinal,
        turn_id,
        item_before,
    })
}

pub(super) fn after_turn(
    project: &SageProjectId,
    chat: &SageChatId,
    turn: &TurnRow,
) -> Result<PageCursor, StoreError> {
    mint(project, chat, turn, None)
}

pub(super) fn before_item(
    project: &SageProjectId,
    chat: &SageChatId,
    turn: &TurnRow,
    item_before: i64,
) -> Result<PageCursor, StoreError> {
    mint(project, chat, turn, Some(item_before))
}

fn mint(
    project: &SageProjectId,
    chat: &SageChatId,
    turn: &TurnRow,
    item_before: Option<i64>,
) -> Result<PageCursor, StoreError> {
    let kind = if item_before.is_some() { "i" } else { "t" };
    let digest = digest_for(
        project,
        chat,
        kind,
        turn.session_ordinal,
        turn.ordinal,
        item_before,
        &turn.id,
    );
    let value = match item_before {
        Some(item) => format!(
            "v2:i:{}:{}:{item}:{digest}",
            turn.session_ordinal, turn.ordinal
        ),
        None => format!("v2:t:{}:{}:{digest}", turn.session_ordinal, turn.ordinal),
    };
    PageCursor::new("cursor", value).map_err(|_| StoreError::Corrupt("page cursor"))
}

fn digest_for(
    project: &SageProjectId,
    chat: &SageChatId,
    kind: &str,
    session_ordinal: i64,
    turn_ordinal: i64,
    item_before: Option<i64>,
    turn_id: &str,
) -> String {
    let mut digest = Sha256::new();
    let session_ordinal = session_ordinal.to_string();
    let turn_ordinal = turn_ordinal.to_string();
    let item_before = item_before
        .map(|value| value.to_string())
        .unwrap_or_default();
    for part in [
        DOMAIN,
        project.as_str().as_bytes(),
        chat.as_str().as_bytes(),
        kind.as_bytes(),
        session_ordinal.as_bytes(),
        turn_ordinal.as_bytes(),
        item_before.as_bytes(),
        turn_id.as_bytes(),
    ] {
        update_part(&mut digest, part);
    }
    format!("{:x}", digest.finalize())
}

fn update_part(digest: &mut Sha256, part: &[u8]) {
    digest.update((part.len() as u64).to_be_bytes());
    digest.update(part);
}

fn counter(value: &str) -> Result<i64, StoreError> {
    let parsed = value.parse::<i64>().map_err(|_| StoreError::Conflict {
        resource: "pageCursor",
    })?;
    if parsed < 0 || parsed.to_string() != value {
        return Err(StoreError::Conflict {
            resource: "pageCursor",
        });
    }
    Ok(parsed)
}
