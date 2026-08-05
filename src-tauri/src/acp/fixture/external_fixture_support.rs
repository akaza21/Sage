use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    acp::{
        contract::{ClientIntentId, RequiredText, SafeText, SageChatId, SageProjectId},
        process::{TreeControl, TreeExit},
        runtime::{BoundedThread, JournalHandle, SessionCoordinator, ThreadWaitError},
        scheduler::Scheduler,
        store::{DatabaseAcpStore, SessionMutation, StartSessionCommand, load_chat_page},
    },
    persistence::Database,
};

use super::external_fixture_adapters::{CommitCheckingPublisher, hex};

pub(super) const FIXTURE_BINDING_DIGEST: [u8; 32] = [7_u8; 32];
const READER_DEADLINE: Duration = Duration::from_secs(5);

pub(super) struct FixtureEnvironment {
    pub(super) project_root: PathBuf,
    pub(super) database_path: PathBuf,
    pub(super) database: Database,
    pub(super) project_id: SageProjectId,
    pub(super) chat_id: SageChatId,
    pub(super) now_millis: i64,
    pub(super) publisher: Arc<CommitCheckingPublisher>,
    pub(super) journal: JournalHandle,
    pub(super) journal_task: tokio::task::JoinHandle<()>,
    pub(super) scheduler: Scheduler,
    pub(super) sessions: SessionCoordinator,
    pub(super) started: SessionMutation,
    pub(super) temp_root: TempRoot,
}

pub(super) async fn prepare_fixture_environment(
    prefix: &str,
    chat_id: &str,
    title: &str,
    start_intent: &str,
) -> FixtureEnvironment {
    let temp_root = TempRoot::new(prefix);
    let project_root = temp_root.path().join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let database_path = temp_root.path().join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let project = database.create_project_from_root(&project_root).unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(chat_id).unwrap();
    let now_millis = crate::persistence::now_millis();
    database
        .connection()
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES (?1,?2,1,?3,0,?4,?4)",
            rusqlite::params![chat_id.as_str(), project_id.as_str(), title, now_millis],
        )
        .unwrap();
    let publisher = Arc::new(CommitCheckingPublisher {
        database: database.clone(),
        terminal_seen_after_commit: Default::default(),
    });
    let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
    let journal_task = runner.spawn();
    let scheduler = Scheduler::new();
    let sessions = SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
    let initial = load_chat_page(
        &database.connection(),
        project_id.clone(),
        chat_id.clone(),
        None,
    )
    .unwrap();
    let started = sessions
        .start_session(StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: initial.projection_revision,
            client_intent_id: ClientIntentId::parse(start_intent).unwrap(),
            binding_id: RequiredText::new("binding", "fixture-binding").unwrap(),
            binding_digest: hex(&FIXTURE_BINDING_DIGEST),
            harness_version: Some(SafeText::new("harnessVersion", "fixture-1").unwrap()),
            model_id: None,
            now_millis,
        })
        .await
        .unwrap();
    FixtureEnvironment {
        project_root,
        database_path,
        database,
        project_id,
        chat_id,
        now_millis,
        publisher,
        journal,
        journal_task,
        scheduler,
        sessions,
        started,
        temp_root,
    }
}

pub(super) struct TempRoot {
    path: PathBuf,
    armed: bool,
}

impl TempRoot {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            path: std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4())),
            armed: true,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn remove(mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
        self.armed = false;
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) struct ProcessCleanup {
    control: TreeControl,
    reader: Option<BoundedThread>,
    zero: bool,
}

impl ProcessCleanup {
    pub(super) fn new(control: TreeControl, reader: Option<BoundedThread>) -> Self {
        Self {
            control,
            reader,
            zero: false,
        }
    }

    pub(super) fn attach_reader(&mut self, reader: BoundedThread) {
        assert!(self.reader.replace(reader).is_none());
    }

    pub(super) fn finish_after_zero(&mut self) -> Result<(), ThreadWaitError> {
        self.zero = true;
        match &mut self.reader {
            Some(reader) => reader.wait(READER_DEADLINE),
            None => Ok(()),
        }
    }

    pub(super) fn wait_zero(&mut self) -> TreeExit {
        let (exit, _) = self
            .control
            .wait_zero()
            .unwrap_or_else(|error| panic!("fixture process did not converge: {error:?}"));
        self.finish_after_zero()
            .unwrap_or_else(|error| panic!("fixture reader did not finish: {error:?}"));
        exit
    }
}

impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        if !self.zero {
            let _ = self.control.request_termination(TreeExit::Cancelled);
            let _ = self.control.wait_zero();
        }
        if let Some(reader) = &mut self.reader {
            let _ = reader.wait(READER_DEADLINE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TempRoot;

    #[test]
    fn temp_root_drop_removes_created_tree() {
        let root = TempRoot::new("sage-fixture-cleanup");
        let path = root.path().to_path_buf();
        std::fs::create_dir_all(&path).unwrap();
        drop(root);
        assert!(!path.exists());
    }
}
