//! Owner-landed fail-closed stub. AGENT-1 replaces the body.

use rusqlite::{OptionalExtension, params};

use crate::acp::contract::{
    EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
};

use super::{StoreError, facts};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentEditDisposition {
    pub(crate) project_id: SageProjectId,
    pub(crate) chat_id: SageChatId,
    pub(crate) session_id: SageAgentSessionId,
    pub(crate) turn_id: SageTurnId,
    pub(crate) expected_revision: EntityRevision,
    pub(crate) state: AgentEditDispositionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AgentEditDispositionState {
    Accepted,
    Rejected,
}

pub(crate) fn record_agent_edit_disposition(
    _disposition: &AgentEditDisposition,
) -> Result<(), String> {
    Err("agent edit disposition requires the durable store connection".into())
}

pub(crate) fn record_agent_edit_disposition_in(
    connection: &mut rusqlite::Connection,
    disposition: &AgentEditDisposition,
    now_millis: i64,
) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    let revision: i64 = transaction
        .query_row(
            "SELECT turns.revision FROM turns
             INNER JOIN agent_sessions ON agent_sessions.id=turns.session_id
             WHERE turns.project_id=?1 AND turns.chat_id=?2 AND turns.session_id=?3
               AND turns.id=?4 AND agent_sessions.project_id=?1 AND agent_sessions.chat_id=?2",
            params![
                disposition.project_id.as_str(),
                disposition.chat_id.as_str(),
                disposition.session_id.as_str(),
                disposition.turn_id.as_str(),
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "turn" })?;
    if facts::entity_revision_from_i64(revision)? != disposition.expected_revision {
        return Err(StoreError::Conflict {
            resource: "turnRevision",
        });
    }
    let existing: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM session_facts
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
           AND kind='agentEditDisposition'",
        params![
            disposition.project_id.as_str(),
            disposition.chat_id.as_str(),
            disposition.session_id.as_str(),
            disposition.turn_id.as_str(),
        ],
        |row| row.get(0),
    )?;
    if existing != 0 {
        return Err(StoreError::Conflict {
            resource: "agentEditDisposition",
        });
    }
    let projection_revision = facts::next_projection_revision(
        &transaction,
        disposition.project_id.as_str(),
        disposition.chat_id.as_str(),
    )?;
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": "agentEditDisposition",
        "entityId": disposition.turn_id.as_str(),
        "state": match disposition.state {
            AgentEditDispositionState::Accepted => "accepted",
            AgentEditDispositionState::Rejected => "rejected",
        },
        "expectedRevision": disposition.expected_revision.get().to_string(),
    }))
    .map_err(|_| StoreError::Corrupt("agent edit disposition serialization"))?;
    facts::append_fact_payload(
        &transaction,
        disposition.project_id.as_str(),
        disposition.chat_id.as_str(),
        disposition.session_id.as_str(),
        Some(disposition.turn_id.as_str()),
        "agentEditDisposition",
        &payload,
        projection_revision,
        now_millis,
    )?;
    facts::publish_projection(
        &transaction,
        disposition.project_id.as_str(),
        disposition.chat_id.as_str(),
        disposition.session_id.as_str(),
        projection_revision,
        now_millis,
    )?;
    transaction.commit().map_err(|_| StoreError::CommitUnknown)
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::acp::contract::{EntityRevision, SageChatId, SageProjectId, SageTurnId};

    #[test]
    fn accept_disposition_records_a_durable_checkpoint_for_the_turn_revision() {
        let mut connection = super::super::extended_tests::setup();
        let session = super::super::start_session(
            &mut connection,
            super::super::extended_tests::start(0, "disposition-session"),
        )
        .unwrap();
        let revision = EntityRevision::new(9_007_199_254_740_993).unwrap();
        connection.execute(
            "INSERT INTO turns VALUES('turn-disposition','project-one','chat-one',?1,1,0,
             'turn-intent','completed','correlated',?2,1,1,1)",
            params![session.session_id.as_str(), revision.get() as i64],
        )
        .unwrap();
        let disposition = AgentEditDisposition {
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: session.session_id.clone(),
            turn_id: SageTurnId::parse("turn-disposition").unwrap(),
            expected_revision: revision,
            state: AgentEditDispositionState::Accepted,
        };
        let stale = AgentEditDisposition {
            expected_revision: EntityRevision::new(9_007_199_254_740_992).unwrap(),
            ..disposition.clone()
        };

        assert!(matches!(
            record_agent_edit_disposition_in(&mut connection, &stale, 2),
            Err(StoreError::Conflict {
                resource: "turnRevision"
            })
        ));
        record_agent_edit_disposition_in(&mut connection, &disposition, 2).unwrap();

        let fact: (String, String, String) = connection
            .query_row(
                "SELECT kind,payload_json,projection_revision FROM session_facts
                 WHERE session_id=?1 AND turn_id=?2",
                params![session.session_id.as_str(), disposition.turn_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(fact.0, "agentEditDisposition");
        assert!(fact.1.contains("9007199254740993"));
        assert_eq!(fact.2, "2");
        assert_eq!(
            connection
                .query_row(
                    "SELECT revision FROM turns WHERE id=?1",
                    [disposition.turn_id.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            revision.get() as i64,
        );
        assert!(matches!(
            record_agent_edit_disposition_in(&mut connection, &disposition, 3),
            Err(StoreError::Conflict {
                resource: "agentEditDisposition"
            })
        ));
    }
}
