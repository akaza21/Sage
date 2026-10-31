//! The lane's command pipe, shared with the frame reader so that the reader
//! can release developer mode's object group when an evaluation answers,
//! as the WebView2 completion callback does, even after the caller stopped
//! waiting for it (ADR-0009b).

use std::{
    collections::HashMap,
    io::Write,
    process::ChildStdin,
    sync::{
        Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

use serde_json::{Value, json};

use crate::acp::browser::BrowserFailure;

#[derive(Default)]
pub(super) struct Wire {
    pub(super) commands: Mutex<Option<ChildStdin>>,
    next_id: AtomicU64,
    /// Developer evaluations in flight, by command id, with their session.
    release_on_answer: Mutex<HashMap<u64, Option<String>>>,
    pub(super) late_releases: AtomicUsize,
}

impl Wire {
    /// Writes one command. With `release_on_answer`, the id is registered
    /// before the bytes leave, so the reader cannot see the answer first.
    pub(super) fn write(
        &self,
        session: Option<&str>,
        method: &str,
        params: Value,
        release_on_answer: bool,
    ) -> Result<u64, BrowserFailure> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        if release_on_answer && let Ok(mut pending) = self.release_on_answer.lock() {
            pending.insert(id, session.map(str::to_owned));
        }
        let mut frame = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            frame["sessionId"] = session.into();
        }
        let mut bytes = frame.to_string().into_bytes();
        bytes.push(0);
        self.commands
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .as_mut()
            .ok_or(BrowserFailure::Unavailable)?
            .write_all(&bytes)
            .map_err(|_| BrowserFailure::Unavailable)?;
        Ok(id)
    }

    /// Called by the reader for every command answer.
    pub(super) fn answered(&self, id: u64) {
        let session = self
            .release_on_answer
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&id));
        if let Some(session) = session {
            let params = serde_json::from_str(&crate::acp::browser::developer::release_params())
                .unwrap_or_default();
            if self
                .write(
                    session.as_deref(),
                    "Runtime.releaseObjectGroup",
                    params,
                    false,
                )
                .is_ok()
            {
                self.late_releases.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}
