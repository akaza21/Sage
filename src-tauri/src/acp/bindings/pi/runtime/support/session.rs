use std::path::{Path, PathBuf};

use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterEvent, AdapterRuntimeError},
    intent::ExternalId,
};

use super::super::{PiRuntime, RequestKind};
use crate::acp::bindings::pi::Operation;

type Reply = (Option<AdapterEvent>, Option<AdapterEgress>);

/// Entries read from Pi's session directory while looking for an older chat's file.
pub(super) const SESSION_DIR_SCAN_LIMIT: usize = 20_000;

impl PiRuntime {
    /// Reopens a chat's Pi session from the file Pi saved it in. Pi's own
    /// default may have changed since; the chat's stored model is put back
    /// through the session-config restore, as any other stored choice.
    pub(in crate::acp::bindings::pi::runtime) fn begin_resume(
        &mut self,
        id: &ExternalId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if !self.initialized || self.external_session.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let path = self
            .session_dir
            .as_deref()
            .and_then(|dir| session_path(dir, id.as_str(), SESSION_DIR_SCAN_LIMIT))
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

/// The file of the session a chat keys by `key`, beside the other sessions of
/// the same working directory. A chat keyed by its file's name opens it
/// directly. Chats from before that key carry Pi's bare id, and Pi names the
/// file `<timestamp>_<id>.jsonl`; those are looked up by a scan that stops
/// after `limit` entries. Pi writes the file once the chat has a message, so
/// a chat that never had one has nothing to reopen.
pub(super) fn session_path(dir: &Path, key: &str, limit: usize) -> Option<PathBuf> {
    if !crate::acp::bindings::pi::protocol::is_session_key(key) {
        return None;
    }
    if key.contains('_') {
        return Some(dir.join(format!("{key}.jsonl"))).filter(|path| path.is_file());
    }
    let name = format!("_{key}.jsonl");
    std::fs::read_dir(dir)
        .ok()?
        .take(limit)
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

#[cfg(test)]
mod tests {
    use super::session_path;

    #[test]
    fn a_keyed_chat_opens_its_file_directly_and_older_chats_scan_within_the_bound() {
        let dir = std::env::temp_dir().join(format!("sage-pi-scan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1_saved.jsonl"), "{}\n").unwrap();
        // No scan at all: even a zero bound finds a keyed chat's file.
        assert_eq!(
            session_path(&dir, "1_saved", 0),
            Some(dir.join("1_saved.jsonl"))
        );
        assert!(session_path(&dir, "2_saved", 0).is_none());
        assert!(session_path(&dir, "saved", 1).is_some());
        assert!(session_path(&dir, "saved", 0).is_none());
        for foreign in ["../1_saved", "1_saved.jsonl", "a/1_saved", ""] {
            assert!(session_path(&dir, foreign, 10).is_none(), "{foreign}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
