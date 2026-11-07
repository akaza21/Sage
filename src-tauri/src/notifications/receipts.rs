use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::{acp::intent::ProjectionChangedV1, error::AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Event {
    Completed,
    Failed,
    Approval,
}

impl Event {
    pub(super) fn body(self) -> &'static str {
        match self {
            Self::Completed => "A turn completed. Open its chat in Sage.",
            Self::Failed => "A turn failed. Open its chat in Sage.",
            Self::Approval => "A turn needs approval. Open its chat in Sage.",
        }
    }
}

/// At most four indexed receipt rows per publication. The journal creates
/// receipts atomically with the exact event, so delayed projection publication
/// cannot miss a completion followed by another Turn or an approval resolution.
/// Claims precede dispatch even while the chat is visible, so refreshes never
/// replay them. An approval is announced only while its request is pending.
pub(super) fn claim(
    connection: &Connection,
    notice: &ProjectionChangedV1,
) -> AppResult<Vec<(i64, Event)>> {
    let mut statement = connection.prepare_cached(
        "SELECT r.id,r.kind,
           (r.request_id IS NULL OR EXISTS(
             SELECT 1 FROM server_requests q
             WHERE q.id=r.request_id AND q.state IN ('received','asked')
           )) FROM turn_notification_receipts r
         JOIN turns t ON t.id=r.turn_id
         WHERE r.session_id=?1 AND r.claimed=0 AND t.project_id=?2 AND r.chat_id=?3
         ORDER BY r.id LIMIT 4",
    )?;
    let rows = statement
        .query_map(
            params![
                notice.session_id.as_str(),
                notice.project_id.as_str(),
                notice.chat_id.as_str()
            ],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut claimed = Vec::with_capacity(rows.len());
    for (id, kind, actionable) in rows {
        if connection.execute(
            "UPDATE turn_notification_receipts SET claimed=1 WHERE id=?1 AND claimed=0",
            [id],
        )? == 1
        {
            let event = match kind.as_str() {
                "completed" => Event::Completed,
                "approval" => Event::Approval,
                _ => Event::Failed,
            };
            if actionable {
                claimed.push((id, event));
            }
        }
    }
    Ok(claimed)
}

/// Marks every unclaimed receipt as handled without announcing it. Used when
/// the process starts and when notices are turned on, so a notice only ever
/// describes an event observed while notices were on in this process.
pub(super) fn retire_pending(connection: &Connection) -> AppResult<usize> {
    Ok(connection.execute(
        "UPDATE turn_notification_receipts SET claimed=1 WHERE claimed=0",
        [],
    )?)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenChat {
    pub project_id: String,
    pub chat_id: String,
}

pub(super) fn target(connection: &Connection, id: i64) -> AppResult<Option<OpenChat>> {
    Ok(connection.query_row(
        "SELECT c.project_id,c.id FROM turn_notification_receipts r JOIN chats c ON c.id=r.chat_id
         JOIN turns t ON t.id=r.turn_id AND t.chat_id=c.id WHERE r.id=?1",
        [id], |row| Ok(OpenChat { project_id:row.get(0)?,chat_id:row.get(1)? }),
    ).optional()?)
}

#[cfg(test)]
mod tests;
