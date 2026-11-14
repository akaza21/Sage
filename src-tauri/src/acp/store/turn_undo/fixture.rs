use rusqlite::params;

use super::*;
use crate::{
    acp::{
        contract::ProjectionRevision,
        store::{load_chat_page, session_surface::is_undoable_turn},
    },
    persistence::Database,
};

pub(crate) const CAPABILITIES: &str = super::super::sessions::PENDING_CAPABILITIES_JSON;

const DIGEST: &str = "c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2c2";

pub(crate) struct Fixture {
    pub(crate) root: std::path::PathBuf,
    pub(crate) database: Database,
    pub(crate) project: SageProjectId,
    pub(crate) chat: SageChatId,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    pub(crate) fn new() -> Self {
        Self::on(crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID)
    }

    pub(crate) fn on(binding: &str) -> Self {
        Self::with_thread(binding, "thread-a", CAPABILITIES)
    }

    pub(crate) fn with_thread(binding: &str, external: &str, capabilities: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sage-undo-{}", uuid::Uuid::new_v4()));
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
                 VALUES('session-a','{project}','{chat}',0,'{binding}','{DIGEST}','2.1.319',
                   'opus','{external}','{capabilities}','ready','cold',0,1,1,1,1,NULL);
                 INSERT INTO acp_session_usage VALUES('{project}','{chat}','session-a',1,0,0,0,0,0,0);
                 INSERT INTO chat_session_heads VALUES('{project}','{chat}','session-a',1,1);
                 INSERT INTO session_runtime_epochs VALUES('{project}','{chat}','session-a',1,
                   'exited',NULL,1,2,'userClose');",
            ))
            .unwrap();
        Self {
            root,
            database,
            project: SageProjectId::parse(project).unwrap(),
            chat: SageChatId::parse(chat).unwrap(),
        }
    }

    pub(crate) fn epoch(&self) -> i64 {
        self.database
            .connection()
            .query_row(
                "SELECT MAX(runtime_epoch) FROM session_runtime_epochs WHERE session_id='session-a'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    pub(crate) fn turn(&self, id: &str, ordinal: i64) {
        let epoch = self.epoch();
        self.database
            .connection()
            .execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,
                   client_intent_id,state,delivery_state,revision,created_at,updated_at,terminal_at)
                 VALUES(?1,?2,?3,'session-a',?6,?4,?1,'completed','correlated',0,?5,?5,?5)",
                params![
                    id,
                    self.project.as_str(),
                    self.chat.as_str(),
                    ordinal,
                    10 + ordinal,
                    epoch
                ],
            )
            .unwrap();
    }

    pub(crate) fn relaunch(&self) {
        let epoch = self.epoch() + 1;
        self.database
            .connection()
            .execute(
                "INSERT INTO session_runtime_epochs VALUES(?1,?2,'session-a',?3,'exited',NULL,3,4,'userClose')",
                params![self.project.as_str(), self.chat.as_str(), epoch],
            )
            .unwrap();
    }

    pub(crate) fn run(&self) {
        let epoch = self.epoch() + 1;
        self.database
            .connection()
            .execute_batch(&format!(
                "UPDATE agent_sessions SET residency='hot' WHERE id='session-a';
                 INSERT INTO session_runtime_epochs
                   VALUES('{}','{}','session-a',{epoch},'running',NULL,3,NULL,NULL);",
                self.project.as_str(),
                self.chat.as_str(),
            ))
            .unwrap();
    }

    pub(crate) fn undo(&self, turn: &str, harness_reverted: bool) -> RecordedUndo {
        self.try_undo(turn, harness_reverted).unwrap()
    }

    pub(crate) fn try_undo(
        &self,
        turn: &str,
        harness_reverted: bool,
    ) -> Result<RecordedUndo, StoreError> {
        // Any other visible turn precedes the undoable latest turn; the page
        // itself lists turns newest first.
        let has_prior = self.page_turns().iter().any(|id| id != turn);
        let command = TurnUndoneCommand {
            project_id: self.project.clone(),
            chat_id: self.chat.clone(),
            session_id: SageAgentSessionId::parse("session-a".to_owned()).unwrap(),
            runtime_epoch: RuntimeEpoch::new(self.epoch() as u64).unwrap(),
            turn_id: SageTurnId::parse(turn.to_owned()).unwrap(),
            harness_reverted,
            cut_at: (has_prior && !harness_reverted)
                .then(|| "00000000-0000-4000-8000-000000000002".into()),
            now_millis: 50,
        };
        record_turn_undone(&mut self.database.connection(), &command)
    }

    pub(crate) fn page_turns(&self) -> Vec<String> {
        load_chat_page(
            &self.database.connection(),
            self.project.clone(),
            self.chat.clone(),
            None,
        )
        .unwrap()
        .turns
        .as_slice()
        .iter()
        .map(|turn| turn.id.as_str().to_owned())
        .collect()
    }

    pub(crate) fn revision(&self) -> ProjectionRevision {
        load_chat_page(
            &self.database.connection(),
            self.project.clone(),
            self.chat.clone(),
            None,
        )
        .unwrap()
        .projection_revision
    }

    pub(crate) fn undoable(&self, turn: &str) -> bool {
        is_undoable_turn(
            &self.database.connection(),
            self.project.as_str(),
            self.chat.as_str(),
            "session-a",
            turn,
        )
        .unwrap()
    }

    pub(crate) fn pending(&self) -> Option<(String, UndoneThread)> {
        pending_thread_rewind(&self.database.connection(), "session-a")
            .unwrap()
            .map(|rewind| (rewind.turn_id.as_str().to_owned(), rewind.thread))
    }
}
