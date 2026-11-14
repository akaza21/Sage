use rusqlite::{Connection, OptionalExtension, params};

use sage_acp_protocol::contract::SageAgentSessionId;
use sage_acp_protocol::session_config::{ConfigId, ConfigMutation};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConfigNotice {
    pub config_id: ConfigId,
    pub saved: ConfigId,
    pub current: ConfigId,
    pub reason: NoticeReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum NoticeReason {
    NotOffered,
    Rejected,
}

impl NoticeReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotOffered => "notOffered",
            Self::Rejected => "rejected",
        }
    }
}

/// Runs in the caller's transaction.
pub(crate) fn set(
    connection: &Connection,
    session: &SageAgentSessionId,
    notice: &SessionConfigNotice,
    now_millis: i64,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO acp_session_config_notices(session_id,config_id,saved,current,reason,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(session_id) DO UPDATE SET config_id=excluded.config_id,
           saved=excluded.saved,current=excluded.current,reason=excluded.reason,
           updated_at=excluded.updated_at",
        params![
            session.as_str(),
            notice.config_id.as_str(),
            notice.saved.as_str(),
            notice.current.as_str(),
            notice.reason.as_str(),
            now_millis
        ],
    )?;
    Ok(())
}

/// Runs in the caller's transaction.
pub(crate) fn clear(
    connection: &Connection,
    session: &SageAgentSessionId,
    choice: &ConfigMutation,
) -> rusqlite::Result<()> {
    let ConfigMutation::Option { config_id, .. } = choice else {
        return Ok(());
    };
    connection.execute(
        "DELETE FROM acp_session_config_notices WHERE session_id=?1 AND config_id=?2",
        params![session.as_str(), config_id.as_str()],
    )?;
    Ok(())
}

/// A row this build cannot read is left out.
pub(crate) fn load(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> rusqlite::Result<Option<SessionConfigNotice>> {
    let row = connection
        .query_row(
            "SELECT config_id,saved,current,reason FROM acp_session_config_notices
             WHERE session_id=?1",
            [session.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(row.and_then(|(config_id, saved, current, reason)| {
        Some(SessionConfigNotice {
            config_id: ConfigId::new("configId", config_id).ok()?,
            saved: ConfigId::new("saved", saved).ok()?,
            current: ConfigId::new("current", current).ok()?,
            reason: match reason.as_str() {
                "notOffered" => NoticeReason::NotOffered,
                "rejected" => NoticeReason::Rejected,
                _ => return None,
            },
        })
    }))
}
