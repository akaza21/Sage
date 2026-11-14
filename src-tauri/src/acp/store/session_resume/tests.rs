use rusqlite::params;

use super::*;
use crate::{
    acp::store::{
        AbortActivationCommand, ActivateSessionCommand, ActivationAbortCategory,
        CompleteActivationCommand, abort_activation, activate_session, complete_activation,
        load_chat_page,
    },
    persistence::Database,
};
use sage_acp_protocol::{
    contract::{ClientIntentId, RuntimeEpoch},
    intent::{ExternalId, NormalizedPayload, ToolCategory},
};

const EARLIER_DIGEST: &str = "e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1";
const CURRENT_DIGEST: &str = "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2";
const ARTIFACT_DIGEST: &str = "a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3";

struct Fixture {
    root: std::path::PathBuf,
    database: Database,
    project: SageProjectId,
    chat: SageChatId,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sage-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.as_ref().unwrap().id.clone();
        let chat = snapshot.active_tab_id.clone().unwrap();
        database
            .connection()
            .execute_batch(&format!(
                "UPDATE chats SET projection_revision=1 WHERE id='{chat}';
                 INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
                   harness_version,model_id,external_session_id,capabilities_json,state,residency,
                   legacy,revision,projection_revision,created_at,updated_at,terminal_at)
                 VALUES('session-earlier','{project}','{chat}',0,'codex-app-server',
                   '{EARLIER_DIGEST}','0.146.0','gpt-earlier','thread-earlier','{capabilities}',
                   'ready','cold',0,1,1,1,1,NULL);
                 INSERT INTO acp_session_usage VALUES('{project}','{chat}','session-earlier',1,0,0,0,0,0,0);
                 INSERT INTO chat_session_heads VALUES('{project}','{chat}','session-earlier',1,1);
                 INSERT INTO session_runtime_epochs VALUES('{project}','{chat}','session-earlier',1,
                   'exited',NULL,1,2,'userClose');",
                capabilities = capabilities("codex-app-server-0.146.0-d3992fec"),
            ))
            .unwrap();
        let fixture = Self {
            root,
            database,
            project: SageProjectId::parse(project).unwrap(),
            chat: SageChatId::parse(chat).unwrap(),
        };
        fixture.turn("turn-earlier", "session-earlier", 0);
        fixture
    }

    fn turn(&self, id: &str, session: &str, ordinal: i64) {
        self.database
            .connection()
            .execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
                   client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
                 VALUES(?1,?2,?3,?4,1,?5,?1,'completed','correlated',0,2,2,2)",
                params![
                    id,
                    self.project.as_str(),
                    self.chat.as_str(),
                    session,
                    ordinal
                ],
            )
            .unwrap();
    }

    fn revision(&self) -> ProjectionRevision {
        load_chat_page(
            &self.database.connection(),
            self.project.clone(),
            self.chat.clone(),
            None,
        )
        .unwrap()
        .projection_revision
    }

    fn link(&self) -> Result<SessionMutation, StoreError> {
        let expected_projection_revision = self.revision();
        link_resumed_session(
            &mut self.database.connection(),
            ResumeEarlierSessionCommand {
                project_id: self.project.clone(),
                chat_id: self.chat.clone(),
                expected_projection_revision,
                earlier_session_id: SageAgentSessionId::parse("session-earlier".to_owned())
                    .unwrap(),
                binding_digest: CURRENT_DIGEST.into(),
                harness_version: SafeText::new(
                    "harnessVersion",
                    sage_acp_protocol::codex_protocol::PINNED_CODEX_VERSION,
                )
                .unwrap(),
                now_millis: 10,
            },
        )
    }

    fn activate(&self, session: &SageAgentSessionId, epoch: u64) {
        let expected_projection_revision = self.revision();
        activate_session(
            &mut self.database.connection(),
            ActivateSessionCommand {
                project_id: self.project.clone(),
                chat_id: self.chat.clone(),
                expected_projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session.clone(),
                runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
                artifact_digest: ARTIFACT_DIGEST.into(),
                now_millis: 20,
            },
        )
        .unwrap();
    }

    fn complete(&self, session: &SageAgentSessionId, thread: &str) -> Result<(), StoreError> {
        complete_activation(
            &mut self.database.connection(),
            CompleteActivationCommand {
                project_id: self.project.clone(),
                chat_id: self.chat.clone(),
                session_id: session.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                binding_digest: CURRENT_DIGEST.into(),
                artifact_digest: ARTIFACT_DIGEST.into(),
                external_session_id: ExternalId::new("externalSessionId", thread).unwrap(),
                capabilities_json: capabilities(
                    sage_acp_protocol::codex_protocol::PINNED_PROTOCOL_IDENTITY,
                ),
                now_millis: 30,
            },
        )
        .map(|_| ())
    }

    fn session_row(&self, id: &str) -> (String, String, Option<String>, Option<String>) {
        self.database
            .connection()
            .query_row(
                "SELECT state,binding_digest,external_session_id,resumes_session_id
                 FROM agent_sessions WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap()
    }

    fn earlier_keeps_its_identity(&self) {
        assert_eq!(
            self.session_row("session-earlier"),
            (
                "closed".into(),
                EARLIER_DIGEST.into(),
                Some("thread-earlier".into()),
                None
            )
        );
    }
}

fn capabilities(protocol: &str) -> String {
    PENDING_CAPABILITIES_JSON.replace("\"pending\"", &format!("\"{protocol}\""))
}

#[test]
fn an_earlier_session_continues_in_a_linked_head_that_fills_the_same_thread() {
    let fixture = Fixture::new();
    let linked = fixture.link().unwrap();
    assert_eq!(linked.disposition, MutationDisposition::Applied);
    assert_eq!(
        crate::acp::store::preceding_session(&fixture.database.connection(), &linked.session_id)
            .unwrap()
            .as_ref()
            .map(SageAgentSessionId::as_str),
        Some("session-earlier")
    );
    let id = linked.session_id.as_str();
    assert_eq!(
        fixture.session_row(id),
        (
            "created".into(),
            CURRENT_DIGEST.into(),
            None,
            Some("session-earlier".into())
        )
    );
    let (ordinal, model): (i64, Option<String>) = fixture
        .database
        .connection()
        .query_row(
            "SELECT ordinal,model_id FROM agent_sessions WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((ordinal, model.as_deref()), (1, Some("gpt-earlier")));
    fixture.earlier_keeps_its_identity();

    let retried = fixture.link().unwrap();
    assert_eq!(retried.session_id, linked.session_id);
    assert_eq!(retried.disposition, MutationDisposition::Duplicate);

    fixture.activate(&linked.session_id, 1);
    assert!(
        fixture
            .complete(&linked.session_id, "thread-other")
            .is_err(),
        "a linked session must fill the earlier thread"
    );
    fixture
        .complete(&linked.session_id, "thread-earlier")
        .unwrap();
    assert_eq!(
        fixture.session_row(id),
        (
            "ready".into(),
            CURRENT_DIGEST.into(),
            Some("thread-earlier".into()),
            Some("session-earlier".into())
        )
    );
    fixture.earlier_keeps_its_identity();
}

#[test]
fn an_aborted_activation_keeps_the_link_for_a_retry_of_the_earlier_session() {
    let fixture = Fixture::new();
    let linked = fixture.link().unwrap();
    fixture.activate(&linked.session_id, 1);
    abort_activation(
        &mut fixture.database.connection(),
        AbortActivationCommand {
            project_id: fixture.project.clone(),
            chat_id: fixture.chat.clone(),
            session_id: linked.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: CURRENT_DIGEST.into(),
            artifact_digest: ARTIFACT_DIGEST.into(),
            category: ActivationAbortCategory::LaunchFailure,
            now_millis: 25,
        },
    )
    .unwrap();
    assert_eq!(
        fixture.session_row(linked.session_id.as_str()),
        (
            "created".into(),
            CURRENT_DIGEST.into(),
            None,
            Some("session-earlier".into())
        )
    );
    let retried = fixture.link().unwrap();
    assert_eq!(retried.session_id, linked.session_id);
    let unstarted = || {
        unstarted_continuation(
            &fixture.database.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str(),
            "session-earlier",
        )
        .unwrap()
        .map(|head| head.session_id)
    };
    assert_eq!(unstarted(), Some(linked.session_id.clone()));
    fixture.activate(&linked.session_id, 2);
    assert_eq!(unstarted(), None);
    fixture.earlier_keeps_its_identity();
}

#[test]
fn history_spans_the_earlier_and_the_linked_session() {
    let fixture = Fixture::new();
    let body = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        user_input: None,
        category: ToolCategory::Execute,
        summary: SafeText::new("summary", "Run command").unwrap(),
        offered_option_kinds: None,
        reason: None,
    })
    .unwrap();
    let digest = super::super::facts::hex_digest(body.as_bytes());
    let (project, chat) = (fixture.project.as_str(), fixture.chat.as_str());
    let connection = fixture.database.connection();
    connection.execute("INSERT INTO items VALUES('item-permission',?1,?2,'session-earlier','turn-earlier',1,0,'primary',NULL,'permissionRequest','completed','declined','external-permission',?3,?4,?5,0,2,2,2)",params![project,chat,body,body.len() as i64,digest]).unwrap();
    connection.execute("INSERT INTO server_requests VALUES('request-earlier',?1,?2,'session-earlier','turn-earlier',1,?3,'external-permission','permission',1,?4,'resolvedDeclined',?5,0,2,2,2)",params![project,chat,EARLIER_DIGEST,digest,body]).unwrap();
    drop(connection);
    let linked = fixture.link().unwrap();
    fixture.activate(&linked.session_id, 1);
    fixture
        .complete(&linked.session_id, "thread-earlier")
        .unwrap();
    fixture.turn("turn-linked", linked.session_id.as_str(), 1);
    let page = load_chat_page(
        &fixture.database.connection(),
        fixture.project.clone(),
        fixture.chat.clone(),
        None,
    )
    .unwrap();
    assert_eq!(page.current_session_id.as_ref(), Some(&linked.session_id));
    let sessions: Vec<_> = page
        .sessions
        .as_slice()
        .iter()
        .map(|session| session.id.as_str().to_owned())
        .collect();
    assert_eq!(
        sessions,
        ["session-earlier", linked.session_id.as_str()].map(str::to_owned)
    );
    let turns: Vec<_> = page
        .turns
        .as_slice()
        .iter()
        .map(|turn| (turn.id.as_str(), turn.session_id.as_str()))
        .collect();
    assert_eq!(
        turns,
        [
            ("turn-linked", linked.session_id.as_str()),
            ("turn-earlier", "session-earlier")
        ]
    );
    assert!(matches!(
        page.turns.as_slice()[1].items.as_slice(),
        [item] if matches!(
            item.content,
            crate::acp::projection::ItemContent::PermissionRequest {
                state: sage_acp_protocol::lifecycle::ServerRequestState::ResolvedDeclined,
                ..
            }
        )
    ));
}

#[test]
fn a_resume_link_names_an_earlier_native_session_of_the_same_chat_and_never_changes() {
    let fixture = Fixture::new();
    let snapshot = fixture.database.bootstrap_snapshot().unwrap();
    let other_chat = fixture
        .database
        .create_chat(&sage_domain::chat::ProjectLayoutRequest {
            project_id: fixture.project.as_str().to_owned(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        })
        .unwrap();
    let insert = |id: &str, chat: &str, state: &str, external: Option<&str>, link: &str| {
        fixture.database.connection().execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
               external_session_id,capabilities_json,state,residency,created_at,updated_at,
               resumes_session_id)
             VALUES(?1,?2,?3,1,'codex-app-server',?4,?5,?6,?7,'cold',0,0,?8)",
            params![
                id,
                fixture.project.as_str(),
                chat,
                CURRENT_DIGEST,
                external,
                PENDING_CAPABILITIES_JSON,
                state,
                link
            ],
        )
    };
    let chat = fixture.chat.as_str();
    for (id, chat, state, external, link) in [
        (
            "foreign",
            other_chat.as_str(),
            "created",
            None,
            "session-earlier",
        ),
        ("unknown", chat, "created", None, "session-missing"),
        (
            "filled",
            chat,
            "ready",
            Some("thread-earlier"),
            "session-earlier",
        ),
        ("self", chat, "created", None, "self"),
    ] {
        let refusal = insert(id, chat, state, external, link).unwrap_err();
        assert!(
            refusal
                .to_string()
                .contains("a resume link must name an earlier native session of the same chat"),
            "{id}: {refusal}"
        );
    }
    let linked = fixture.link().unwrap();
    for update in [
        "UPDATE agent_sessions SET resumes_session_id=NULL WHERE id=?1",
        "UPDATE agent_sessions SET resumes_session_id=?1 WHERE id='session-earlier'",
    ] {
        let refusal = fixture
            .database
            .connection()
            .execute(update, [linked.session_id.as_str()])
            .unwrap_err();
        assert!(
            refusal
                .to_string()
                .contains("agent session resume link is immutable"),
            "{update}: {refusal}"
        );
    }
}
