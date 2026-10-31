use crate::acp::contract::{ProjectGeneration, SageChatId, SageProjectId};
use rusqlite::OptionalExtension;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

pub(crate) mod ipc;
mod output;
mod stream;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix;
#[cfg(windows)]
mod windows;

use output::TerminalOutputSink;
pub use output::{TerminalOutputPage, TerminalOutputSnapshot};

pub const MAX_PER_CHAT: usize = 4;
pub const MAX_PER_PROJECT: usize = 16;
pub const MAX_GLOBAL: usize = 16;
pub const OUTPUT_BYTES: usize = 1_048_576;
pub const OUTPUT_LINES: usize = 4_096;
pub const INPUT_BYTES: usize = 4_096;
pub const OUTPUT_CHUNK_BYTES: usize = 8_192;
pub const OUTPUT_PAGE_BYTES: usize = 48_000;
/// Largest grid a terminal may be created or resized to.
pub const MAX_COLUMNS: u16 = 500;
pub const MAX_ROWS: u16 = 300;

fn validate_dimensions(columns: u16, rows: u16) -> Result<(), TerminalError> {
    if (1..=MAX_COLUMNS).contains(&columns) && (1..=MAX_ROWS).contains(&rows) {
        Ok(())
    } else {
        Err(TerminalError::InvalidDimensions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerminalSessionId(uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalRevision(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalHandle {
    pub id: TerminalSessionId,
    pub revision: TerminalRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalError {
    Capacity,
    InvalidScope,
    InvalidDimensions,
    InvalidInput,
    UnknownSession,
    Ended,
    Busy,
    Backend,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TerminalScope {
    pub project: SageProjectId,
    pub generation: ProjectGeneration,
    pub chat: SageChatId,
}

pub struct TerminalAuthority<'a>(&'a crate::state::AppState);

trait TerminalProcess: Send {
    fn shell_path(&self) -> &str;
    fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError>;
    fn resize(&mut self, columns: u16, rows: u16) -> Result<(), TerminalError>;
    fn terminate(&mut self) -> Result<(), TerminalError>;
}

enum TerminalSpawnFailure {
    Clean(TerminalError),
    #[cfg_attr(
        not(any(windows, target_os = "linux", target_os = "macos")),
        allow(dead_code, reason = "only the PTY backends retain a failed spawn")
    )]
    Retained(Box<dyn TerminalProcess>),
}

impl From<TerminalError> for TerminalSpawnFailure {
    fn from(error: TerminalError) -> Self {
        Self::Clean(error)
    }
}

trait TerminalFactory: Send + Sync {
    fn spawn(
        &self,
        root: &crate::project_registry::ProjectRootAuthority,
        columns: u16,
        rows: u16,
        output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalSpawnFailure>;
}

pub struct TerminalSessionPort {
    factory: Box<dyn TerminalFactory>,
    sessions: Mutex<HashMap<TerminalSessionId, Session>>,
}

impl std::fmt::Debug for TerminalSessionPort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSessionPort")
            .finish_non_exhaustive()
    }
}

impl Drop for TerminalSessionPort {
    fn drop(&mut self) {
        let _ = self.shutdown();
        let sessions = self
            .sessions
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, session) in sessions.drain() {
            std::mem::forget(session);
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
struct UnsupportedFactory;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
impl TerminalFactory for UnsupportedFactory {
    fn spawn(
        &self,
        _root: &crate::project_registry::ProjectRootAuthority,
        _columns: u16,
        _rows: u16,
        _output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalSpawnFailure> {
        Err(TerminalError::Backend.into())
    }
}

struct Session {
    scope: TerminalScope,
    revision: TerminalRevision,
    output: TerminalOutputSink,
    reader_busy: Arc<AtomicBool>,
    process: Box<dyn TerminalProcess>,
}

/// Whether `scope` names a chat tab of its project, read off the async runtime.
async fn chat_tab_exists(
    database: &crate::persistence::Database,
    scope: &TerminalScope,
) -> Result<bool, TerminalError> {
    let database = database.clone();
    let project = scope.project.clone();
    let chat = scope.chat.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database
            .connection()
            .query_row(
                "SELECT 1 FROM chat_tabs WHERE project_id=?1 AND chat_id=?2 LIMIT 1",
                [project.as_str(), chat.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
    })
    .await
    .map_err(|_| TerminalError::Backend)?
    .map(|row| row.is_some())
    .map_err(|_| TerminalError::InvalidScope)
}

impl TerminalSessionPort {
    fn shell_path(&self, handle: TerminalHandle) -> Result<String, TerminalError> {
        let sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        Ok(session.process.shell_path().to_owned())
    }

    /// Whether another terminal fits the global, project and chat limits.
    fn admits(&self, scope: &TerminalScope) -> Result<bool, TerminalError> {
        let sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let (project, chat) = sessions.values().fold((0, 0), |(project, chat), session| {
            let same_project = session.scope.project == scope.project;
            (
                project + usize::from(same_project),
                chat + usize::from(same_project && session.scope == *scope),
            )
        });
        Ok(sessions.len() < MAX_GLOBAL && project < MAX_PER_PROJECT && chat < MAX_PER_CHAT)
    }

    fn new(factory: Box<dyn TerminalFactory>) -> Self {
        Self {
            factory,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn desktop() -> Self {
        #[cfg(windows)]
        {
            Self::new(Box::new(windows::ConPtyFactory))
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Self::new(Box::new(unix::UnixPtyFactory))
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            Self::new(Box::new(UnsupportedFactory))
        }
    }

    pub async fn create(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        columns: u16,
        rows: u16,
    ) -> Result<TerminalHandle, TerminalError> {
        let state = authority.0;
        validate_dimensions(columns, rows)?;
        let context = state
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        if !chat_tab_exists(&state.database, scope).await? {
            return Err(TerminalError::InvalidScope);
        }
        if !self.admits(scope)? {
            return Err(TerminalError::Capacity);
        }
        // Spawning a shell can take a while (ConPTY on Windows), so the
        // session table stays unlocked for reads, writes and resizes of the
        // other terminals meanwhile. `context` holds the project transition
        // lock, so no other create can take the slot admitted above.
        let root = state
            .database
            .selected_project_root_authority(&context)
            .map_err(|_| TerminalError::InvalidScope)?;
        root.revalidate().map_err(|_| TerminalError::InvalidScope)?;
        let output = TerminalOutputSink::new();
        let (process, cleanup_unknown) =
            match self.factory.spawn(&root, columns, rows, output.clone()) {
                Ok(process) => (process, false),
                Err(TerminalSpawnFailure::Clean(error)) => return Err(error),
                Err(TerminalSpawnFailure::Retained(process)) => (process, true),
            };
        let id = TerminalSessionId(uuid::Uuid::new_v4());
        let revision = TerminalRevision(1);
        self.sessions
            .lock()
            .map_err(|_| TerminalError::Backend)?
            .insert(
                id,
                Session {
                    scope: scope.clone(),
                    revision,
                    output,
                    reader_busy: Arc::new(AtomicBool::new(false)),
                    process,
                },
            );
        if cleanup_unknown {
            Err(TerminalError::Backend)
        } else {
            Ok(TerminalHandle { id, revision })
        }
    }

    pub async fn write_input(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
        text: &str,
    ) -> Result<(), TerminalError> {
        let state = authority.0;
        if text.is_empty() || text.len() > INPUT_BYTES {
            return Err(TerminalError::InvalidInput);
        }
        let _context = state
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        let mut sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get_mut(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        if session.scope != *scope || session.revision != handle.revision {
            return Err(TerminalError::InvalidScope);
        }
        session.process.write(text.as_bytes())
    }

    pub async fn resize(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
        columns: u16,
        rows: u16,
    ) -> Result<(), TerminalError> {
        let state = authority.0;
        validate_dimensions(columns, rows)?;
        let _context = state
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        let mut sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get_mut(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        if session.scope != *scope || session.revision != handle.revision {
            return Err(TerminalError::InvalidScope);
        }
        session.process.resize(columns, rows)
    }

    pub async fn output_snapshot(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
    ) -> Result<TerminalOutputSnapshot, TerminalError> {
        let state = authority.0;
        let _context = state
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        let sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        if session.scope != *scope || session.revision != handle.revision {
            return Err(TerminalError::InvalidScope);
        }
        session.output.snapshot()
    }

    pub async fn close(
        &self,
        authority: &TerminalAuthority<'_>,
        scope: &TerminalScope,
        handle: TerminalHandle,
    ) -> Result<(), TerminalError> {
        let state = authority.0;
        let _context = state
            .lock_active_project(&scope.project, scope.generation)
            .await
            .map_err(|_| TerminalError::InvalidScope)?;
        let mut sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let session = sessions
            .get_mut(&handle.id)
            .ok_or(TerminalError::UnknownSession)?;
        if session.scope != *scope || session.revision != handle.revision {
            return Err(TerminalError::InvalidScope);
        }
        session.process.terminate()?;
        session.output.wake();
        sessions.remove(&handle.id);
        Ok(())
    }

    pub fn close_chat(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Result<(), TerminalError> {
        self.terminate_matching(|scope| &scope.project == project && &scope.chat == chat)
    }

    pub fn project_changed(
        &self,
        project: &SageProjectId,
        generation: ProjectGeneration,
    ) -> Result<(), TerminalError> {
        self.terminate_matching(|scope| &scope.project != project || scope.generation != generation)
    }

    pub fn shutdown(&self) -> Result<(), TerminalError> {
        self.terminate_matching(|_| true)
    }

    fn terminate_matching(
        &self,
        predicate: impl Fn(&TerminalScope) -> bool,
    ) -> Result<(), TerminalError> {
        let mut sessions = self.sessions.lock().map_err(|_| TerminalError::Backend)?;
        let ids = sessions
            .iter()
            .filter(|(_, session)| predicate(&session.scope))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        let mut failed = false;
        for id in ids {
            if let Some(session) = sessions.get_mut(&id) {
                if session.process.terminate().is_ok() {
                    session.output.wake();
                    sessions.remove(&id);
                } else {
                    failed = true;
                }
            }
        }
        if failed {
            Err(TerminalError::Backend)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
