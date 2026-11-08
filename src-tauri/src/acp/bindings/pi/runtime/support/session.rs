use std::path::{Path, PathBuf};

use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
    intent::{ExternalId, ModelId},
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

type Reply = (Option<AdapterEvent>, Option<AdapterEgress>);

impl PiRuntime {
    /// Reopens a chat's Pi session from the file Pi saved it in.
    pub(in crate::acp::bindings::pi::runtime) fn begin_resume(
        &mut self,
        id: &ExternalId,
        model: Option<&ModelId>,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self.initialized || self.external_session.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        if model.is_some_and(|model| self.current_model.as_deref() != Some(model.as_str())) {
            return Err(AdapterRuntimeError::UnsupportedCommand);
        }
        let path = self
            .session_dir
            .as_deref()
            .and_then(|dir| session_path(dir, id.as_str()))
            .and_then(|path| path.into_os_string().into_string().ok())
            .ok_or(AdapterRuntimeError::UnsupportedCommand)?;
        self.begin_one(
            Operation::Resume {
                session_id: id.as_str().to_owned(),
                path,
            },
            RequestKind::ResumeSession,
        )
    }

    pub(super) fn open_session(&mut self, id: String) -> Result<Reply, AdapterRuntimeError> {
        let external =
            ExternalId::new("piSessionId", id).map_err(|_| AdapterRuntimeError::Frame)?;
        self.commands = std::sync::Arc::default();
        self.commands_pending = false;
        self.commands_discovered = false;
        self.external_session = Some(external.clone());
        self.recent_cancelled_dialogs.clear();
        self.available_thinking.clear();
        Ok((
            Some(AdapterEvent::SessionReady {
                id: self.identity(external, None, None),
                loaded: false,
            }),
            None,
        ))
    }
}

/// Pi names a session's file `<timestamp>_<id>.jsonl`, beside the other
/// sessions of the same working directory. Pi writes it once the chat has a
/// message, so a chat that never had one has nothing to reopen.
fn session_path(dir: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let name = format!("_{id}.jsonl");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|file| file.ends_with(&name))
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
        .map(|entry| entry.path())
}
