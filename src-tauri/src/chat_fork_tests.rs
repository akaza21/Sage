use rusqlite::params;

use super::*;
use crate::acp::store::{chat_fork::fork_source, load_chat_page};
use crate::native_session_import::ImportChatStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_acp_protocol::{
    contract::{SageChatId, SageProjectId, SageTurnId},
    intent::ExternalId,
};
use sage_db::database::Database;
use sage_domain::chat::ProjectLayoutRequest;

/// A capability snapshot of the pinned Codex binding that can fork.
fn forkable() -> String {
    format!(
        r#"{{"schemaVersion":1,"protocolVersion":"{identity}","newSession":true,"resumeSession":true,"loadSession":false,"closeSession":true,"prompt":true,"cancel":true,"nativeSessionFork":true,"activeTurnSteer":true,"childConversationProjection":false,"childTranscriptRead":true,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}}"#,
        identity = sage_acp_protocol::codex_protocol::PINNED_PROTOCOL_IDENTITY
    )
}

struct Fixture {
    root: std::path::PathBuf,
    database: Database,
    project: String,
    parent: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    /// A Project whose first Chat has a hot, idle Codex head session with
    /// one completed Turn, as an answered Chat looks before it is forked.
    fn new(capabilities: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sage-fork-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.as_ref().unwrap().id.clone();
        let parent = snapshot.active_tab_id.clone().unwrap();
        database
            .connection()
            .execute_batch(&format!(
                "UPDATE chats SET title='Chat: Inspect repo files',projection_revision=1 WHERE id='{parent}';
                 INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
                   harness_version,model_id,external_session_id,capabilities_json,state,residency,
                   legacy,revision,projection_revision,created_at,updated_at,terminal_at)
                 VALUES('session-parent','{project}','{parent}',0,'codex-app-server','{digest}',
                   '{version}',NULL,'thread-parent','{capabilities}','active','hot',0,1,1,1,1,NULL);
                 INSERT INTO acp_session_usage VALUES('{project}','{parent}','session-parent',1,0,0,0,0,0,0);
                 INSERT INTO chat_session_heads VALUES('{project}','{parent}','session-parent',1,1);
                 INSERT INTO session_runtime_epochs VALUES('{project}','{parent}','session-parent',1,
                   'running',NULL,1,NULL,NULL);",
                digest = "a".repeat(64),
                version = sage_acp_protocol::codex_protocol::PINNED_CODEX_VERSION,
            ))
            .unwrap();
        let fixture = Self {
            root,
            database,
            project,
            parent,
        };
        fixture.turn("turn-1", 0, "completed");
        fixture
    }

    fn turn(&self, id: &str, ordinal: i64, state: &str) {
        let terminal = (state == "completed").then_some(2_i64);
        self.database
            .connection()
            .execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
                   client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
                 VALUES(?1,?2,?3,'session-parent',1,?4,?1,?5,'correlated',0,2,2,?6)",
                params![id, self.project, self.parent, ordinal, state, terminal],
            )
            .unwrap();
    }

    fn ids(&self) -> (SageProjectId, SageChatId) {
        (
            SageProjectId::parse(self.project.clone()).unwrap(),
            SageChatId::parse(self.parent.clone()).unwrap(),
        )
    }

    fn source(&self, turn: &str) -> Result<ForkSource, crate::acp::store::StoreError> {
        let (project, chat) = self.ids();
        fork_source(
            &self.database.connection(),
            &project,
            &chat,
            &SageTurnId::parse(turn.to_owned()).unwrap(),
        )
    }

    fn layout(&self) -> ProjectLayoutRequest {
        let snapshot = self.database.bootstrap_snapshot().unwrap();
        ProjectLayoutRequest {
            project_id: self.project.clone(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        }
    }

    fn fork(&self, thread: &str) -> Result<String, ForkChatError> {
        let source = self.source("turn-1").expect("forkable source");
        let external = ExternalId::new("externalSessionId", thread.to_owned()).unwrap();
        self.database
            .create_fork_chat(&self.layout(), &source, &external)
    }
}

#[test]
fn the_fork_title_is_the_parent_title_with_the_next_free_number() {
    let none: [&str; 0] = [];
    assert_eq!(fork_title("Chat: X", none.iter().copied()), "Chat: X (2)");
    assert_eq!(
        fork_title("Chat: X", ["Chat: X", "Chat: X (2)"].into_iter()),
        "Chat: X (3)"
    );
    assert_eq!(
        fork_title("Chat: X", ["Chat: X (3)"].into_iter()),
        "Chat: X (2)"
    );
    assert_eq!(
        fork_title("Chat: X (2)", ["Chat: X (2)"].into_iter()),
        "Chat: X (2) (2)"
    );
    let long = format!("Chat: {}é", "a".repeat(124));
    let title = fork_title(&long, none.iter().copied());
    let name = title.strip_prefix("Chat: ").unwrap();
    assert!(name.len() <= MAX_CHAT_TITLE_BYTES && name.ends_with(" (2)"));
    assert!(crate::chat_lifecycle::canonical_chat_title(name).is_ok());
}

#[test]
fn a_fork_keeps_its_derived_title_when_its_harness_names_the_thread() {
    let fixture = Fixture::new(&forkable());
    let fork = fixture.fork("thread-fork").expect("fork chat");
    let mut connection = fixture.database.connection();
    let transaction = connection.transaction().unwrap();
    crate::chat_lifecycle::apply_harness_title_in(
        &transaction,
        &fixture.project,
        &fork,
        "Inspect repo files",
        9,
    )
    .unwrap();
    transaction.commit().unwrap();
    let (title, source): (String, String) = connection
        .query_row(
            "SELECT title,title_source FROM chats WHERE id=?1",
            [&fork],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        (title.as_str(), source.as_str()),
        ("Chat: Inspect repo files (2)", "fork")
    );
}

#[test]
fn a_fork_is_a_persistent_chat_above_its_parent_with_rust_owned_lineage() {
    let fixture = Fixture::new(&forkable());
    let parent_position: i64 = fixture
        .database
        .connection()
        .query_row(
            "SELECT position FROM workbench_tabs WHERE id=?1",
            [&fixture.parent],
            |row| row.get(0),
        )
        .unwrap();
    let fork = fixture.fork("thread-fork").expect("fork chat");
    let connection = fixture.database.connection();
    let (title, tab_title, position): (String, String, i64) = connection
        .query_row(
            "SELECT c.title,w.title,w.position FROM chats c JOIN workbench_tabs w ON w.id=c.id
             WHERE c.id=?1",
            [&fork],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(title, "Chat: Inspect repo files (2)");
    assert_eq!(tab_title, title);
    assert_eq!(position, parent_position);
    let moved: i64 = connection
        .query_row(
            "SELECT position FROM workbench_tabs WHERE id=?1",
            [&fixture.parent],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(moved, parent_position + 1);
    let active: Option<String> = connection
        .query_row(
            "SELECT active_tab_id FROM project_layouts WHERE project_id=?1",
            [&fixture.project],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active.as_deref(), Some(fork.as_str()));
    let lineage: (String, String, String, String) = connection
        .query_row(
            "SELECT parent_chat_id,source_session_id,source_turn_id,relation FROM chat_lineage
             WHERE child_chat_id=?1",
            [&fork],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        lineage,
        (
            fixture.parent.clone(),
            "session-parent".into(),
            "turn-1".into(),
            "nativeFork".into()
        )
    );
    let session: (String, String, String, String, String) = connection
        .query_row(
            "SELECT s.state,s.residency,s.external_session_id,s.capabilities_json,s.binding_id
             FROM chat_session_heads h JOIN agent_sessions s ON s.id=h.session_id
             WHERE h.chat_id=?1",
            [&fork],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        session,
        (
            "ready".into(),
            "cold".into(),
            "thread-fork".into(),
            forkable(),
            "codex-app-server".into()
        )
    );
    let page = load_chat_page(
        &connection,
        SageProjectId::parse(fixture.project.clone()).unwrap(),
        SageChatId::parse(fork.clone()).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        page.forked_from.as_ref().map(SageChatId::as_str),
        Some(fixture.parent.as_str())
    );
    assert!(page.turns.as_slice().is_empty());
    let parent_page = load_chat_page(
        &connection,
        SageProjectId::parse(fixture.project.clone()).unwrap(),
        SageChatId::parse(fixture.parent.clone()).unwrap(),
        None,
    )
    .unwrap();
    assert!(parent_page.forked_from.is_none());
    let encoded = serde_json::to_value(&page).unwrap();
    assert_eq!(encoded["forkedFrom"], fixture.parent.as_str());
    assert!(
        serde_json::to_value(&parent_page)
            .unwrap()
            .get("forkedFrom")
            .is_none()
    );
    drop(connection);

    let second = fixture.fork("thread-fork-2").expect("second fork");
    let title: String = fixture
        .database
        .connection()
        .query_row("SELECT title FROM chats WHERE id=?1", [&second], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(title, "Chat: Inspect repo files (3)");
}

#[test]
fn fork_fails_closed_without_the_capability_or_an_idle_completed_anchor() {
    let unforkable = Fixture::new(&forkable().replace(
        r#""nativeSessionFork":true"#,
        r#""nativeSessionFork":false"#,
    ));
    assert!(unforkable.source("turn-1").is_err());

    let fixture = Fixture::new(&forkable());
    assert!(fixture.source("turn-1").is_ok());
    assert!(fixture.source("turn-missing").is_err());
    fixture.turn("turn-2", 1, "running");
    assert!(
        fixture.source("turn-1").is_err(),
        "a later Turn is in flight"
    );
    assert!(
        fixture.source("turn-2").is_err(),
        "the anchor is not completed"
    );

    let cold = Fixture::new(&forkable());
    cold.database
        .connection()
        .execute_batch(
            "UPDATE session_runtime_epochs SET state='exited',terminal_at=3,exit_kind='parked';
             UPDATE agent_sessions SET state='ready',residency='cold';",
        )
        .unwrap();
    assert!(
        cold.source("turn-1").is_err(),
        "a parked runtime cannot fork"
    );

    let (project, _) = fixture.ids();
    assert!(
        fork_source(
            &fixture.database.connection(),
            &project,
            &SageChatId::parse("side-temporary").unwrap(),
            &SageTurnId::parse("turn-1".to_owned()).unwrap(),
        )
        .is_err()
    );
}

#[test]
fn a_turn_started_during_the_native_fork_refuses_the_fork_chat() {
    let fixture = Fixture::new(&forkable());
    let source = fixture.source("turn-1").unwrap();
    fixture.turn("turn-2", 1, "running");
    let external = ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap();
    assert!(matches!(
        fixture
            .database
            .create_fork_chat(&fixture.layout(), &source, &external),
        Err(ForkChatError::Refused(_))
    ));
    let chats: i64 = fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM chats", [], |row| row.get(0))
        .unwrap();
    assert_eq!(chats, 1, "the refused fork left no Chat behind");
}

#[test]
fn layout_and_tab_limit_refusals_happen_before_the_commit() {
    let fixture = Fixture::new(&forkable());
    let source = fixture.source("turn-1").unwrap();
    let external = ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap();
    let mut foreign = fixture.layout();
    foreign.tabs.retain(|tab| tab.id != fixture.parent);
    assert!(matches!(
        fixture
            .database
            .create_fork_chat(&foreign, &source, &external),
        Err(ForkChatError::Refused(_))
    ));
}

#[test]
fn a_failed_commit_is_commit_unknown_and_never_a_refusal() {
    let fixture = Fixture::new(&forkable());
    let source = fixture.source("turn-1").unwrap();
    let external = ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap();
    // The COMMIT itself fails after every pre-commit check has passed.
    let layout = fixture.layout();
    fixture
        .database
        .connection()
        .commit_hook(Some(|| true))
        .unwrap();
    let outcome = fixture
        .database
        .create_fork_chat(&layout, &source, &external);
    fixture
        .database
        .connection()
        .commit_hook(None::<fn() -> bool>)
        .unwrap();
    assert!(
        matches!(outcome, Err(ForkChatError::CommitUnknown(_))),
        "{outcome:?}"
    );
}

#[test]
fn only_a_refusal_before_the_commit_deletes_the_native_fork() {
    assert_eq!(release_after(&ForkCommit::Committed), Some(true));
    assert_eq!(
        release_after(&ForkCommit::Refused("stale".into())),
        Some(false)
    );
    // A failed COMMIT or a lost join may have landed: release nothing.
    assert_eq!(release_after(&ForkCommit::Unknown), None);
}

#[test]
fn a_fork_starts_with_the_config_choices_of_its_parent() {
    use crate::acp::session_config::choices;
    use sage_acp_protocol::{
        contract::SageAgentSessionId,
        session_config::{ConfigId, ConfigMutation, ConfigValue},
    };
    let fixture = Fixture::new(&forkable());
    let parent = SageAgentSessionId::parse("session-parent").unwrap();
    let mode = ConfigMutation::Mode(ConfigId::new("modeId", "plan").unwrap());
    let effort = ConfigMutation::Option {
        config_id: ConfigId::new("configId", "effort").unwrap(),
        value: ConfigValue::Select {
            value: ConfigId::new("value", "high").unwrap(),
        },
    };
    {
        let connection = fixture.database.connection();
        assert!(choices::record(&connection, &parent, &mode, 1).unwrap());
        assert!(choices::record(&connection, &parent, &effort, 2).unwrap());
    }
    let fork = fixture.fork("thread-fork").expect("fork chat");
    let connection = fixture.database.connection();
    let session: String = connection
        .query_row(
            "SELECT session_id FROM chat_session_heads WHERE chat_id=?1",
            [&fork],
            |row| row.get(0),
        )
        .unwrap();
    let session = SageAgentSessionId::parse(session).unwrap();
    assert_ne!(session, parent);
    assert_eq!(
        choices::load(&connection, &session).unwrap(),
        [mode, effort]
    );
}

#[test]
fn an_imported_session_becomes_a_resumable_chat_once_per_thread() {
    use crate::acp::store::native_import::{import_lister, imported_session, imported_threads};
    let fixture = Fixture::new(&forkable());
    let (project, _) = fixture.ids();
    assert!(
        import_lister(&fixture.database.connection(), &project, None)
            .unwrap()
            .is_none(),
        "a runtime without the capability does not list sessions"
    );
    let importable = forkable().replace(
        r#""nativeSessionFork":true"#,
        r#""nativeSessionFork":true,"nativeSessionImport":true"#,
    );
    let fixture = Fixture::new(&importable);
    let (project, parent) = fixture.ids();
    let lister = import_lister(&fixture.database.connection(), &project, None)
        .unwrap()
        .expect("the hot Codex chat lists sessions");
    assert_eq!(lister.chat_id, parent);
    let history = r#"{"turns":[],"retentionLimited":false}"#;
    let import = |thread: &str| {
        let thread = ExternalId::new("externalSessionId", thread.to_owned()).unwrap();
        fixture.database.create_import_chat(
            &fixture.layout(),
            &lister,
            &thread,
            "  Fix the\n build ",
            history,
        )
    };
    let chat = import("thread-cli").expect("import");
    let connection = fixture.database.connection();
    let row: (String, String, String, String, String, String) = connection
        .query_row(
            "SELECT c.title,c.title_source,s.state,s.residency,s.external_session_id,
                    s.capabilities_json
             FROM chats c JOIN agent_sessions s ON s.chat_id=c.id WHERE c.id=?1",
            [&chat],
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
        .unwrap();
    assert_eq!(
        row,
        (
            "Chat: Fix the build".into(),
            "harness".into(),
            "ready".into(),
            "cold".into(),
            "thread-cli".into(),
            importable.clone()
        )
    );
    let chat_id = SageChatId::parse(chat).unwrap();
    let page = load_chat_page(&connection, project.clone(), chat_id.clone(), None).unwrap();
    assert!(page.imported_history && page.turns.as_slice().is_empty());
    assert_eq!(
        serde_json::to_value(&page).unwrap()["importedHistory"],
        true
    );
    assert!(
        !load_chat_page(&connection, project.clone(), parent, None)
            .unwrap()
            .imported_history
    );
    let imported = imported_session(&connection, &project, &chat_id, true)
        .unwrap()
        .unwrap();
    assert_eq!(imported.history_json.as_deref(), Some(history));
    assert_eq!(imported.runtime_epoch, 1, "shown under its first runtime");
    assert_eq!(
        imported_threads(&connection, &project).unwrap(),
        ["thread-cli"]
    );
    drop(connection);
    for held in ["thread-cli", "thread-parent"] {
        assert!(
            import(held).is_err(),
            "{held} is already held by a Sage session"
        );
    }
}

#[test]
fn a_listing_leaves_out_every_thread_the_project_holds() {
    use crate::acp::store::native_import::{
        HELD_THREADS, held_threads, import_lister, imported_threads,
    };
    let importable = forkable().replace(
        r#""nativeSessionFork":true"#,
        r#""nativeSessionFork":true,"nativeSessionImport":true"#,
    );
    let fixture = Fixture::new(&importable);
    let (project, _) = fixture.ids();
    let lister = import_lister(&fixture.database.connection(), &project, None)
        .unwrap()
        .expect("the hot Codex chat lists sessions");
    let thread = ExternalId::new("externalSessionId", "thread-cli".to_owned()).unwrap();
    fixture
        .database
        .create_import_chat(
            &fixture.layout(),
            &lister,
            &thread,
            "Session",
            r#"{"turns":[],"retentionLimited":false}"#,
        )
        .expect("import");
    let connection = fixture.database.connection();
    // The harness is asked to leave out only imported threads, but an
    // import refuses every thread a Project session holds, so a page is
    // filtered against all of them.
    assert_eq!(
        imported_threads(&connection, &project).unwrap(),
        ["thread-cli"]
    );
    let held = held_threads(
        &connection,
        &project,
        &["thread-cli", "thread-parent", "thread-new"],
    )
    .unwrap();
    assert_eq!(
        held,
        ["thread-cli", "thread-parent"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    // Each listed thread is an indexed lookup, never a scan of the Project.
    let plan = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {HELD_THREADS}"))
        .unwrap()
        .query_map(rusqlite::params![project.as_str(), "[]"], |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("idx_agent_sessions_external_session"),
        "{plan}"
    );
}

#[test]
fn an_import_reads_only_on_the_runtime_that_listed_it() {
    use crate::acp::store::native_import::import_lister;
    use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId};
    let importable = forkable().replace(
        r#""nativeSessionFork":true"#,
        r#""nativeSessionFork":true,"nativeSessionImport":true"#,
    );
    let fixture = Fixture::new(&importable);
    let (project, parent) = fixture.ids();
    // A second hot runtime, more recently active than the parent's.
    let other = fixture.fork("thread-other").expect("fork chat");
    fixture
        .database
        .connection()
        .execute_batch(&format!(
            "UPDATE agent_sessions SET residency='hot',updated_at=9 WHERE chat_id='{other}';
             INSERT INTO session_runtime_epochs SELECT project_id,chat_id,id,1,'running',
               NULL,1,NULL,NULL FROM agent_sessions WHERE chat_id='{other}';"
        ))
        .unwrap();
    let connection = fixture.database.connection();
    let newest = import_lister(&connection, &project, None).unwrap().unwrap();
    assert_eq!(newest.chat_id.as_str(), other);
    let session = SageAgentSessionId::parse("session-parent").unwrap();
    let epoch = RuntimeEpoch::new(1).unwrap();
    let pinned = import_lister(&connection, &project, Some((&session, epoch)))
        .unwrap()
        .expect("the listing runtime still reads");
    assert_eq!(pinned.chat_id, parent);
    let restarted = RuntimeEpoch::new(2).unwrap();
    assert!(
        import_lister(&connection, &project, Some((&session, restarted)))
            .unwrap()
            .is_none(),
        "a restarted runtime never listed the session"
    );
}
