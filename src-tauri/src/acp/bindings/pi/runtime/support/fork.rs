//! Forking a Pi chat through Sage's Branch action. Pi's `clone` copies the
//! session at its current position and moves the process onto the copy, so
//! the runtime reads the copy's file with `get_state` and moves back onto
//! its own session with `switch_session`. The fork chat then reopens the
//! copy in its own runtime, as any saved Pi chat reopens.

use serde_json::Value;

use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterRuntimeError},
        pi::{NativeCommand, NativeRequest, Operation, session_file_key},
    },
    goal::NativeRpcReply,
    intent::ExternalId,
};

use super::super::{PiRuntime, RequestKind};
use super::session::{SESSION_DIR_SCAN_LIMIT, session_path};

/// Forks a runtime holds until their chats commit or their copies are removed.
/// A full ledger refuses the next fork rather than make a copy it cannot track.
const MAX_TRACKED_FORKS: usize = 8;

/// Only a copy known to have no Chat may be retried for deletion.
pub(in crate::acp::bindings::pi::runtime) struct ForkCopy {
    key: ExternalId,
    cleanup_pending: bool,
}

/// A fork under way: its step, and the Pi command id that step waits on.
#[derive(Debug)]
pub(in crate::acp::bindings::pi::runtime) struct Fork {
    step: ForkStep,
    command_id: String,
}

/// Each step keeps the path of the chat's own session file (`home`), which
/// Pi is moved back onto and which a copy must never be mistaken for.
#[derive(Debug)]
enum ForkStep {
    Cloning { home: String },
    Reading { home: String },
    Returning { home: String, fork: Option<String> },
}

impl PiRuntime {
    /// Starts a fork of the idle session through its latest reply.
    pub(in crate::acp::bindings::pi::runtime) fn begin_fork(
        &mut self,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        if self.fork.is_some() || self.active_turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        self.retry_fork_cleanup();
        if self.forks.len() >= MAX_TRACKED_FORKS {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let home = self.home_path().ok_or(AdapterRuntimeError::Operation)?;
        let egress = self.begin_native(NativeRequest::Clone)?;
        let command_id = self.command_id(&egress)?;
        self.fork = Some(Fork {
            step: ForkStep::Cloning { home },
            command_id,
        });
        Ok(egress)
    }

    /// The file of the chat's own session, which Pi writes once it has a
    /// message; a session with none has nothing to fork.
    fn home_path(&self) -> Option<String> {
        let dir = self.session_dir.as_deref()?;
        let key = self.external_session.as_ref()?;
        session_path(dir, key.as_str(), SESSION_DIR_SCAN_LIMIT)?
            .into_os_string()
            .into_string()
            .ok()
    }

    /// Whether `command_id` is the request the fork under way waits on.
    pub(super) fn is_fork_step(&self, command_id: &str) -> bool {
        self.fork
            .as_ref()
            .is_some_and(|fork| fork.command_id == command_id)
    }

    /// Takes the next fork step from Pi's answer. Pi must always be moved
    /// back onto the chat's session, whether or not the caller still waits,
    /// so each step is sent regardless; only the last one answers the
    /// caller. A Pi that cannot be moved back leaves the runtime unusable.
    pub(super) fn fork_reply(
        &mut self,
        command: NativeCommand,
        data: Option<Value>,
        awaited: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let fork = self.fork.take().ok_or(AdapterRuntimeError::Frame)?;
        let cancelled = data.as_ref().map(|data| data["cancelled"].as_bool());
        let (step, request) = match (fork.step, command) {
            (ForkStep::Cloning { home }, NativeCommand::Clone) => match cancelled {
                // Pi refused or cancelled the clone and stayed where it was.
                None | Some(Some(true)) => {
                    return Ok(self.answer_fork(awaited, NativeRpcReply::Rejected));
                }
                Some(Some(false)) => (ForkStep::Reading { home }, NativeRequest::ForkState),
                Some(None) => return Err(self.lost_fork()),
            },
            (ForkStep::Reading { home }, NativeCommand::ForkState) => {
                // A failed `get_state` leaves the copy's file unknown: Pi's
                // `clone` answer does not name it, and the session folder is
                // shared with every other Pi chat, so no file can be safely
                // picked as the copy. Pi is still moved back and the fork is
                // refused; that copy stays in Pi's own session list.
                let fork = data.as_ref().and_then(fork_key);
                let path = home.clone();
                (
                    ForkStep::Returning { home, fork },
                    NativeRequest::ReturnToSession { path },
                )
            }
            (ForkStep::Returning { home, fork }, NativeCommand::ReturnToSession) => {
                if cancelled != Some(Some(false)) {
                    return Err(self.lost_fork());
                }
                return Ok(self.settle_fork(&home, fork, awaited));
            }
            _ => return Err(self.lost_fork()),
        };
        let Ok(egress) = self.begin_one(Operation::Native(request), RequestKind::Native) else {
            return Err(self.lost_fork());
        };
        let Ok(command_id) = self.command_id(&egress) else {
            return Err(self.lost_fork());
        };
        if awaited {
            self.native_pending = Some(command_id.clone());
        }
        self.fork = Some(Fork { step, command_id });
        Ok(Some(egress))
    }

    /// Pi is back on the chat's session: the copy it reported becomes the
    /// fork, unless it is the chat's own file or no caller wants it.
    fn settle_fork(
        &mut self,
        home: &str,
        fork: Option<String>,
        awaited: bool,
    ) -> Option<AdapterEgress> {
        let home_key = session_file_key(home);
        let Some(fork) = fork
            .filter(|fork| Some(fork.as_str()) != home_key)
            .and_then(|fork| ExternalId::new("externalSessionId", fork).ok())
        else {
            return self.answer_fork(awaited, NativeRpcReply::Rejected);
        };
        self.forks.push(ForkCopy {
            key: fork.clone(),
            cleanup_pending: false,
        });
        if !awaited {
            // No chat will open a copy its caller gave up on.
            self.cleanup_fork_copy(self.forks.len() - 1);
            return None;
        }
        self.answer_fork(true, NativeRpcReply::Forked(fork))
    }

    fn answer_fork(&mut self, awaited: bool, reply: NativeRpcReply) -> Option<AdapterEgress> {
        if awaited {
            self.native_rpc_reply = Some(reply);
        }
        None
    }

    /// Pi may be left on the copy: the chat's session can no longer be
    /// trusted to be the one Pi runs, so the runtime stops here.
    fn lost_fork(&mut self) -> AdapterRuntimeError {
        self.fork = None;
        self.poison();
        AdapterRuntimeError::Operation
    }

    /// A fork step that never reached Pi. Before the clone Pi is still on
    /// the chat's session; after it, Pi may be on the copy.
    pub(super) fn fork_step_dropped(&mut self, command_id: &str) {
        if !self.is_fork_step(command_id) {
            return;
        }
        if matches!(
            self.fork.as_ref().map(|fork| &fork.step),
            Some(ForkStep::Cloning { .. })
        ) {
            self.fork = None;
        } else {
            self.lost_fork();
        }
    }

    /// Lets go of a fork this runtime made. A committed fork is its chat's
    /// now; a refused one leaves only the copy's file, which is removed so
    /// Pi does not list a chat Sage never opened. Failed deletions still take
    /// a ledger slot and are retried before the next fork can clone.
    pub(in crate::acp::bindings::pi::runtime) fn release_fork_copy(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let index = self
            .forks
            .iter()
            .position(|known| &known.key == fork)
            .ok_or(AdapterRuntimeError::Operation)?;
        let released = if committed {
            self.forks.remove(index);
            true
        } else {
            self.cleanup_fork_copy(index)
        };
        self.native_rpc_reply = Some(if released {
            NativeRpcReply::Released
        } else {
            NativeRpcReply::Rejected
        });
        Ok(None)
    }

    /// One pass over at most eight entries per fork attempt; no retry queue
    /// or background task can outlive this runtime or grow beyond its ledger.
    fn retry_fork_cleanup(&mut self) {
        for index in (0..self.forks.len()).rev() {
            if self.forks[index].cleanup_pending {
                self.cleanup_fork_copy(index);
            }
        }
    }

    fn cleanup_fork_copy(&mut self, index: usize) -> bool {
        if self.remove_copy(&self.forks[index].key) {
            self.forks.remove(index);
            true
        } else {
            self.forks[index].cleanup_pending = true;
            tracing::warn!("a refused Pi fork's session copy could not be removed");
            false
        }
    }

    #[cfg(not(test))]
    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    #[cfg(test)]
    fn remove_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        (self.remove_file_for_test)(path)
    }

    /// Removes the saved copy of a fork no chat holds; true once no such
    /// file is left. The key names a file inside Pi's session folder only.
    fn remove_copy(&self, fork: &ExternalId) -> bool {
        let Some(dir) = self.session_dir.as_deref() else {
            return false;
        };
        // Fork keys come from `session_file_key`, so they name the exact
        // file in this directory. Do not use an existence probe: it would
        // mistake inaccessible metadata for a copy that is already gone.
        let file = dir.join(format!("{}.jsonl", fork.as_str()));
        match self.remove_file(&file) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

/// The chat key of the copy Pi moved onto, from `get_state`. Only a saved
/// copy can be reopened, so a bare session id does not count.
fn fork_key(state: &Value) -> Option<String> {
    session_file_key(state.get("sessionFile")?.as_str()?).map(str::to_owned)
}
