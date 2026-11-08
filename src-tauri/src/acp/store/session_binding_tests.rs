use rusqlite::Connection;

use super::super::{
    ActivateSessionCommand, CompleteActivationCommand, MutationDisposition, ParkSessionCommand,
    StoreError, activate_session, complete_activation,
    extended_tests::{capabilities, setup, start},
    load_chat_page, park_session_after_zero_process, start_session,
};
use super::*;
use crate::acp::{contract::RequiredText, intent::ExternalId};

const STARTED_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const NODE_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const STARTED_ARTIFACT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn project() -> SageProjectId {
    SageProjectId::parse("project-one").unwrap()
}

fn chat() -> SageChatId {
    SageChatId::parse("chat-one").unwrap()
}

fn version(value: &str) -> BindingVersion {
    SafeText::new("harnessVersion", value).unwrap()
}

fn revision(connection: &Connection) -> ProjectionRevision {
    load_chat_page(connection, project(), chat(), None)
        .unwrap()
        .projection_revision
}

/// Activates `epoch`, commits it ready (optionally proving `change`), and
/// parks it cold again.
fn run_epoch(
    connection: &mut Connection,
    session: &SageAgentSessionId,
    epoch: u64,
    change: Option<BindingChange>,
) -> Result<(), StoreError> {
    let runtime_epoch = RuntimeEpoch::new(epoch).unwrap();
    let artifact = change
        .as_ref()
        .map_or(STARTED_ARTIFACT.to_owned(), |change| {
            change.to_artifact.clone()
        });
    activate_session(
        connection,
        ActivateSessionCommand {
            project_id: project(),
            chat_id: chat(),
            expected_projection_revision: revision(connection),
            client_intent_id: ClientIntentId::parse(format!("activate-{epoch}")).unwrap(),
            session_id: session.clone(),
            runtime_epoch,
            artifact_digest: artifact.clone(),
            now_millis: 20,
        },
    )?;
    complete_activation(
        connection,
        CompleteActivationCommand {
            binding_change: change,
            project_id: project(),
            chat_id: chat(),
            session_id: session.clone(),
            runtime_epoch,
            binding_digest: STARTED_DIGEST.into(),
            artifact_digest: artifact,
            external_session_id: ExternalId::new("external", "native-session").unwrap(),
            capabilities_json: capabilities(),
            now_millis: 25,
        },
    )?;
    park_session_after_zero_process(
        connection,
        ParkSessionCommand {
            project_id: project(),
            chat_id: chat(),
            session_id: session.clone(),
            runtime_epoch,
            exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
            now_millis: 30,
        },
    )?;
    Ok(())
}

fn node_update() -> BindingChange {
    BindingChange {
        kind: BindingChangeKind::NodeOnly,
        from_digest: STARTED_DIGEST.into(),
        to_digest: NODE_DIGEST.into(),
        to_artifact: STARTED_ARTIFACT.into(),
        from_version: Some(version("1.0.0")),
        to_version: version("1.0.0"),
    }
}

fn notice(connection: &Connection) -> Option<SessionBindingNotice> {
    load_chat_page(connection, project(), chat(), None)
        .unwrap()
        .sessions
        .as_slice()[0]
        .binding_notice
        .clone()
}

fn started_session(connection: &mut Connection) -> SageAgentSessionId {
    let started = start_session(connection, start(0, "start-drift")).unwrap();
    run_epoch(connection, &started.session_id, 1, None).unwrap();
    started.session_id
}

#[test]
fn a_session_that_never_drifted_records_the_build_it_started_on() {
    let mut connection = setup();
    let session = started_session(&mut connection);
    assert_eq!(
        recorded_build(&connection, &session).unwrap(),
        RecordedBuild {
            digest: STARTED_DIGEST.into(),
            harness_version: Some("1.0.0".into()),
            artifact_digest: Some(STARTED_ARTIFACT.into()),
        }
    );
    assert_eq!(notice(&connection), None);
}

#[test]
fn a_compatible_drift_continues_and_is_noticed_once_on_its_epoch() {
    let mut connection = setup();
    let session = started_session(&mut connection);
    run_epoch(&mut connection, &session, 2, Some(node_update())).unwrap();
    let continued = Some(SessionBindingNotice::Continued {
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        change: BindingChangeKind::NodeOnly,
        from_version: Some(version("1.0.0")),
        to_version: version("1.0.0"),
    });
    assert_eq!(notice(&connection), continued);
    assert_eq!(
        recorded_build(&connection, &session).unwrap().digest,
        NODE_DIGEST
    );
    // The next activation on the same build records nothing new: the notice
    // stays anchored to the epoch that crossed the drift.
    run_epoch(&mut connection, &session, 3, None).unwrap();
    assert_eq!(notice(&connection), continued);
}

#[test]
fn a_drift_classified_from_a_superseded_build_is_not_recorded() {
    let mut connection = setup();
    let session = started_session(&mut connection);
    run_epoch(&mut connection, &session, 2, Some(node_update())).unwrap();
    // Classified against the original build, which is no longer current.
    let stale = BindingChange {
        to_digest: "c".repeat(64),
        ..node_update()
    };
    assert!(matches!(
        run_epoch(&mut connection, &session, 3, Some(stale)),
        Err(StoreError::Conflict {
            resource: "sessionBinding"
        })
    ));
    assert_eq!(
        recorded_build(&connection, &session).unwrap().digest,
        NODE_DIGEST
    );
}

#[test]
fn an_incompatible_build_ends_the_session_and_keeps_the_chat_for_a_new_one() {
    let mut connection = setup();
    let session = started_session(&mut connection);
    let refusal = RefuseSessionBindingCommand {
        project_id: project(),
        chat_id: chat(),
        session_id: session.clone(),
        expected_projection_revision: revision(&connection),
        client_intent_id: ClientIntentId::parse("refuse-drift").unwrap(),
        reason: BindingRefusalReason::UnknownBuild,
        from_version: Some(version("1.0.0")),
        to_version: version("1.1.0"),
        now_millis: 40,
    };
    let refused = refuse_session_binding(&mut connection, refusal.clone()).unwrap();
    assert_eq!(
        (refused.state, refused.disposition),
        (SessionState::Incompatible, MutationDisposition::Applied)
    );
    let page = load_chat_page(&connection, project(), chat(), None).unwrap();
    assert_eq!(page.current_session_id, None);
    assert_eq!(
        page.sessions.as_slice()[0].binding_notice,
        Some(SessionBindingNotice::Refused {
            reason: BindingRefusalReason::UnknownBuild,
            from_version: Some(version("1.0.0")),
            to_version: version("1.1.0"),
        })
    );
    assert_eq!(
        refuse_session_binding(&mut connection, refusal)
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    // The Chat accepts the next session at the following ordinal.
    let expected = revision(&connection).get();
    let next = start_session(&mut connection, start(expected, "start-after-refusal")).unwrap();
    assert_ne!(next.session_id, session);
}

#[test]
fn the_refusal_respects_the_durable_session_lifecycle() {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let mut connection = database.connection();
    connection
        .execute_batch(&format!(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Drift',1,1,1);
             INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Drift',0,1,1);
             INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,harness_version,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','{STARTED_DIGEST}','1.0.0','external-session','{capabilities}','ready','cold',0,0,0,1,1);
             INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
             INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at,terminal_at,exit_kind) VALUES('project-one','chat-one','session-one',1,'exited','{STARTED_ARTIFACT}',1,2,'idleAfterTurn');
             INSERT INTO acp_project_usage VALUES('project-one',1,1,0,0,0,0,0);
             INSERT INTO acp_session_usage VALUES('project-one','chat-one','session-one',0,0,0,0,0,0,0);
             UPDATE acp_store_usage SET chat_count=1,session_count=1 WHERE singleton=1;",
            capabilities = capabilities(),
        ))
        .unwrap();
    let session = SageAgentSessionId::parse("session-one").unwrap();
    let refused = refuse_session_binding(
        &mut connection,
        RefuseSessionBindingCommand {
            project_id: project(),
            chat_id: chat(),
            session_id: session.clone(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("refuse-real-schema").unwrap(),
            reason: BindingRefusalReason::OtherFamily,
            from_version: Some(version("1.0.0")),
            to_version: version("2.0.0"),
            now_millis: 50,
        },
    )
    .unwrap();
    assert_eq!(refused.state, SessionState::Incompatible);
    let row: (String, Option<i64>, i64) = connection
        .query_row(
            "SELECT state,terminal_at,(SELECT COUNT(*) FROM chat_session_heads)
             FROM agent_sessions WHERE id='session-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(row, ("incompatible".into(), Some(50), 0));
    assert_eq!(
        recorded_build(&connection, &session)
            .unwrap()
            .artifact_digest,
        Some(STARTED_ARTIFACT.into())
    );
}
