//! Session binding drift: which exact build a session currently runs under,
//! the durable record of a proven-compatible drift, and the durable refusal
//! of an incompatible one.
//!
//! `agent_sessions.binding_digest` is the session's write scope and never
//! changes, so every scoped write and foreign key keeps its meaning. The
//! exact build a runtime epoch ran under lives in the session's facts: a
//! `sessionBindingChanged` fact is committed in the same transaction as the
//! activation it was proven by, and names that epoch.

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::acp::{
    contract::{
        ClientIntentId, ProjectionRevision, RuntimeEpoch, SafeText, SageAgentSessionId, SageChatId,
        SageProjectId,
    },
    lifecycle::SessionState,
};

use super::{
    MutationDisposition, StoreError,
    facts::{append_fact_payload, entity_revision_from_i64, next_projection_revision},
    sessions::{SessionMutation, ensure_one, update_chat, valid_digest, validate_chat_revision},
};

const CHANGED_FACT: &str = "sessionBindingChanged";
const REFUSED_FACT: &str = "sessionBindingRefused";
/// Harness versions are stored as `SafeText<128>` on the session row.
pub(crate) const BINDING_VERSION_BYTES: usize = 128;
type BindingVersion = SafeText<BINDING_VERSION_BYTES>;

/// A binding change a session continues across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingChangeKind {
    /// Same harness build; only the runtime (Node.js) changed.
    NodeOnly,
    /// A different characterised build in the same compatibility family.
    CompatibleAdapter,
}

/// Why a session cannot continue on the admitted build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BindingRefusalReason {
    /// The admitted or the recorded build is not in the curated table.
    UnknownBuild,
    /// Both builds are characterised but belong to different families.
    OtherFamily,
}

/// A classified compatible drift, committed only with a ready activation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingChange {
    pub kind: BindingChangeKind,
    pub from_digest: String,
    pub to_digest: String,
    pub to_artifact: String,
    pub from_version: Option<BindingVersion>,
    pub to_version: BindingVersion,
}

/// The exact build a session last ran under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedBuild {
    pub digest: String,
    pub harness_version: Option<String>,
    /// `None` when no epoch of the current build has reached ready.
    pub artifact_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RefuseSessionBindingCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub reason: BindingRefusalReason,
    pub from_version: Option<BindingVersion>,
    pub to_version: BindingVersion,
    pub now_millis: i64,
}

/// What the Chat shows about a session's latest binding decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionBindingNotice {
    Continued {
        runtime_epoch: RuntimeEpoch,
        change: BindingChangeKind,
        from_version: Option<BindingVersion>,
        to_version: BindingVersion,
    },
    Refused {
        reason: BindingRefusalReason,
        from_version: Option<BindingVersion>,
        to_version: BindingVersion,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChangedFact {
    runtime_epoch: RuntimeEpoch,
    change: BindingChangeKind,
    from_version: Option<BindingVersion>,
    to_version: BindingVersion,
    from_digest: String,
    to_digest: String,
    to_artifact: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RefusedFact {
    reason: BindingRefusalReason,
    from_version: Option<BindingVersion>,
    to_version: BindingVersion,
}

fn latest_fact(
    connection: &Connection,
    session: &str,
    kinds: &[&str],
) -> Result<Option<(String, String)>, StoreError> {
    let (first, second) = match kinds {
        [only] => (*only, *only),
        [first, second] => (*first, *second),
        _ => return Err(StoreError::Corrupt("binding fact kinds")),
    };
    Ok(connection
        .query_row(
            "SELECT kind,payload_json FROM session_facts
             WHERE session_id=?1 AND kind IN (?2,?3) ORDER BY sequence DESC LIMIT 1",
            params![session, first, second],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn changed_fact(payload: &str) -> Result<ChangedFact, StoreError> {
    serde_json::from_str(payload).map_err(|_| StoreError::Corrupt("binding change fact"))
}

/// The exact build the session last ran under: the latest committed drift,
/// or the build it was started with.
pub(crate) fn recorded_build(
    connection: &Connection,
    session: &SageAgentSessionId,
) -> Result<RecordedBuild, StoreError> {
    if let Some((_, payload)) = latest_fact(connection, session.as_str(), &[CHANGED_FACT])? {
        let fact = changed_fact(&payload)?;
        return Ok(RecordedBuild {
            digest: fact.to_digest,
            harness_version: Some(fact.to_version.as_str().to_owned()),
            artifact_digest: Some(fact.to_artifact),
        });
    }
    let (digest, harness_version): (String, Option<String>) = connection
        .query_row(
            "SELECT binding_digest,harness_version FROM agent_sessions WHERE id=?1",
            [session.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "session",
        })?;
    // Without a drift, every epoch that reached ready ran this exact build.
    let artifact_digest = connection
        .query_row(
            "SELECT artifact_digest FROM session_runtime_epochs
             WHERE session_id=?1 AND artifact_digest IS NOT NULL
               AND state IN ('running','terminating','exited')
             ORDER BY runtime_epoch DESC LIMIT 1",
            [session.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(RecordedBuild {
        digest,
        harness_version,
        artifact_digest,
    })
}

/// Records the proven-compatible drift of a ready-activation commit, if it
/// carries one. The session must still be on the build the drift was
/// classified from.
pub(super) fn record_binding_change(
    transaction: &Transaction<'_>,
    command: &super::CompleteActivationCommand,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    let Some(change) = &command.binding_change else {
        return Ok(());
    };
    let (project, chat, session) = (&command.project_id, &command.chat_id, &command.session_id);
    let (runtime_epoch, now_millis) = (command.runtime_epoch, command.now_millis);
    if ![&change.from_digest, &change.to_digest, &change.to_artifact]
        .into_iter()
        .all(|digest| valid_digest(digest))
    {
        return Err(StoreError::Corrupt("binding change digest"));
    }
    if recorded_build(transaction, session)?.digest != change.from_digest {
        return Err(StoreError::Conflict {
            resource: "sessionBinding",
        });
    }
    let payload = serde_json::to_string(&ChangedFact {
        runtime_epoch,
        change: change.kind,
        from_version: change.from_version.clone(),
        to_version: change.to_version.clone(),
        from_digest: change.from_digest.clone(),
        to_digest: change.to_digest.clone(),
        to_artifact: change.to_artifact.clone(),
    })
    .map_err(|_| StoreError::Corrupt("binding change fact"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        None,
        CHANGED_FACT,
        &payload,
        projection,
        now_millis,
    )
}

/// Ends a cold session whose recorded build cannot continue on the admitted
/// one. The Chat keeps every Turn; its head is cleared so the next prompt
/// starts a new session in the same Chat.
pub(crate) fn refuse_session_binding(
    connection: &mut Connection,
    command: RefuseSessionBindingCommand,
) -> Result<SessionMutation, StoreError> {
    let transaction = connection.transaction()?;
    let (state, residency, revision, projection_revision, open_epoch, head): (
        String,
        String,
        i64,
        i64,
        bool,
        bool,
    ) = transaction
        .query_row(
            "SELECT s.state,s.residency,s.revision,s.projection_revision,
                    EXISTS(SELECT 1 FROM session_runtime_epochs e WHERE e.session_id=s.id
                           AND e.state IN ('starting','running','terminating')),
                    EXISTS(SELECT 1 FROM chat_session_heads h WHERE h.session_id=s.id)
             FROM agent_sessions s WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str()
            ],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "session",
        })?;
    if state == "incompatible"
        && latest_fact(&transaction, command.session_id.as_str(), &[REFUSED_FACT])?.is_some()
    {
        return Ok(SessionMutation {
            session_id: command.session_id,
            runtime_epoch: None,
            state: SessionState::Incompatible,
            revision: entity_revision_from_i64(revision)?,
            projection_revision: ProjectionRevision::new(projection_revision as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            disposition: MutationDisposition::Duplicate,
        });
    }
    validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    if !matches!(state.as_str(), "created" | "ready") || residency != "cold" || open_epoch || !head
    {
        return Err(StoreError::InvalidTransition {
            resource: "sessionBinding",
        });
    }
    let projection = next_projection_revision(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
    )?;
    let next = entity_revision_from_i64(revision)?
        .next()
        .map_err(|_| StoreError::CounterExhausted)?;
    let session = command.session_id.as_str();
    ensure_one(
        transaction.execute(
            "DELETE FROM chat_session_heads WHERE session_id=?1",
            [session],
        )?,
        "chat session head",
    )?;
    // The lifecycle reaches `incompatible` only through an activation
    // attempt (`starting` or `recovering`); this refusal is one that never
    // launched a process.
    let attempt = if state == "created" {
        "starting"
    } else {
        "recovering"
    };
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state=?2 WHERE id=?1",
            params![session, attempt],
        )?,
        "session",
    )?;
    ensure_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='incompatible',revision=?2,projection_revision=?3,
             updated_at=?4,terminal_at=?4 WHERE id=?1 AND state=?5",
            params![
                session,
                next.get() as i64,
                projection.get() as i64,
                command.now_millis,
                attempt
            ],
        )?,
        "session",
    )?;
    let payload = serde_json::to_string(&RefusedFact {
        reason: command.reason,
        from_version: command.from_version.clone(),
        to_version: command.to_version.clone(),
    })
    .map_err(|_| StoreError::Corrupt("binding refusal fact"))?;
    append_fact_payload(
        &transaction,
        command.project_id.as_str(),
        command.chat_id.as_str(),
        session,
        None,
        REFUSED_FACT,
        &payload,
        projection,
        command.now_millis,
    )?;
    update_chat(
        &transaction,
        &command.project_id,
        &command.chat_id,
        projection,
        command.now_millis,
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(SessionMutation {
        session_id: command.session_id,
        runtime_epoch: None,
        state: SessionState::Incompatible,
        revision: next,
        projection_revision: projection,
        disposition: MutationDisposition::Applied,
    })
}

/// The latest binding decision for one session, for the Chat projection.
pub(super) fn binding_notice(
    connection: &Connection,
    session: &str,
) -> Result<Option<SessionBindingNotice>, StoreError> {
    let Some((kind, payload)) = latest_fact(connection, session, &[CHANGED_FACT, REFUSED_FACT])?
    else {
        return Ok(None);
    };
    Ok(Some(if kind == CHANGED_FACT {
        let fact = changed_fact(&payload)?;
        SessionBindingNotice::Continued {
            runtime_epoch: fact.runtime_epoch,
            change: fact.change,
            from_version: fact.from_version,
            to_version: fact.to_version,
        }
    } else {
        let fact: RefusedFact = serde_json::from_str(&payload)
            .map_err(|_| StoreError::Corrupt("binding refusal fact"))?;
        SessionBindingNotice::Refused {
            reason: fact.reason,
            from_version: fact.from_version,
            to_version: fact.to_version,
        }
    }))
}

#[cfg(test)]
#[path = "session_binding_tests.rs"]
mod tests;
