//! Pi's `clone` copies the session at its current position and moves the process onto the
//! copy, so the runtime reads the copy's file with `get_state` and moves back onto its own
//! session with `switch_session`.

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

/// A full ledger refuses the next fork rather than make a copy it cannot track.
const MAX_TRACKED_FORKS: usize = 8;

/// Only a copy known to have no Chat may be retried for deletion.
pub(in crate::acp::bindings::pi::runtime) struct ForkCopy {
    /// An unidentified copy holds its slot for the rest of this runtime.
    key: Option<ExternalId>,
    cleanup_pending: bool,
}

#[derive(Debug)]
pub(in crate::acp::bindings::pi::runtime) struct Fork {
    step: ForkStep,
    command_id: String,
}

#[derive(Debug, Clone)]
enum ForkStep {
    Cloning { home: String },
    Reading { home: String },
    Returning { home: String, fork: Option<String> },
}

impl Drop for PiRuntime {
    fn drop(&mut self) {
        // Recovery can outlive the process. Retry refused-copy deletions
        // once when ownership ends, including EOF before a return reply.
        self.retry_fork_cleanup();
        if let Some(Fork {
            step: ForkStep::Returning { home, fork },
            ..
        }) = self.fork.take()
        {
            let _ = self.settle_fork(&home, fork, false);
        }
    }
}

impl PiRuntime {
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

    fn home_path(&self) -> Option<String> {
        let dir = self.session_dir.as_deref()?;
        let key = self.external_session.as_ref()?;
        session_path(dir, key.as_str(), SESSION_DIR_SCAN_LIMIT)?
            .into_os_string()
            .into_string()
            .ok()
    }

    pub(super) fn is_fork_step(&self, command_id: &str) -> bool {
        self.fork
            .as_ref()
            .is_some_and(|fork| fork.command_id == command_id)
    }

    /// Pi must always be moved back onto the chat's session, whether or not the caller still waits,
    /// so each step is sent regardless.
    pub(super) fn fork_reply(
        &mut self,
        command: NativeCommand,
        data: Option<Value>,
        awaited: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let step = self
            .fork
            .as_ref()
            .ok_or(AdapterRuntimeError::Frame)?
            .step
            .clone();
        let cancelled = data.as_ref().map(|data| data["cancelled"].as_bool());
        let (step, request) = match (step, command) {
            (ForkStep::Cloning { home }, NativeCommand::Clone) => match cancelled {
                // Pi refused or cancelled the clone and stayed where it was.
                None | Some(Some(true)) => {
                    self.fork = None;
                    return Ok(self.answer_fork(awaited, NativeRpcReply::Rejected));
                }
                Some(Some(false)) => (ForkStep::Reading { home }, NativeRequest::ForkState),
                Some(None) => return Err(self.lost_fork()),
            },
            (ForkStep::Reading { home }, NativeCommand::ForkState) => {
                // A failed `get_state` leaves the copy's file unknown: Pi's
                // `clone` answer does not name it, and the session folder is
                // shared with every other Pi chat, so no file can be safely
                // picked as the copy.
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
                self.fork = None;
                return Ok(self.settle_fork(&home, fork, awaited));
            }
            _ => return Err(self.lost_fork()),
        };
        // A failed return must not discard the only record of Pi's clone.
        self.fork.as_mut().ok_or(AdapterRuntimeError::Frame)?.step = step;
        let Ok(egress) = self.begin_one(Operation::Native(request), RequestKind::Native) else {
            return Err(self.lost_fork());
        };
        let Ok(command_id) = self.command_id(&egress) else {
            return Err(self.lost_fork());
        };
        if awaited {
            self.native_pending = Some(command_id.clone());
        }
        self.fork
            .as_mut()
            .ok_or(AdapterRuntimeError::Frame)?
            .command_id = command_id;
        Ok(Some(egress))
    }

    fn settle_fork(
        &mut self,
        home: &str,
        fork: Option<String>,
        awaited: bool,
    ) -> Option<AdapterEgress> {
        let home_key = session_file_key(home);
        let fork = fork
            .filter(|fork| Some(fork.as_str()) != home_key)
            .and_then(|fork| ExternalId::new("externalSessionId", fork).ok());
        self.forks.push(ForkCopy {
            key: fork.clone(),
            cleanup_pending: false,
        });
        let Some(fork) = fork else {
            return self.answer_fork(awaited, NativeRpcReply::Rejected);
        };
        if !awaited {
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
        if let Some(fork) = self.fork.take() {
            if let ForkStep::Returning { home, fork } = fork.step {
                let _ = self.settle_fork(&home, fork, false);
            } else {
                // Pi may have cloned before we could read the copy's name.
                self.forks.push(ForkCopy {
                    key: None,
                    cleanup_pending: false,
                });
            }
        }
        self.native_pending = None;
        self.poison();
        AdapterRuntimeError::Operation
    }

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

    pub(in crate::acp::bindings::pi::runtime) fn release_fork_copy(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let index = self
            .forks
            .iter()
            .position(|known| known.key.as_ref() == Some(fork))
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

    fn retry_fork_cleanup(&mut self) {
        for index in (0..self.forks.len()).rev() {
            if self.forks[index].cleanup_pending {
                self.cleanup_fork_copy(index);
            }
        }
    }

    fn cleanup_fork_copy(&mut self, index: usize) -> bool {
        let Some(key) = self.forks[index].key.as_ref() else {
            return false;
        };
        if self.remove_copy(key) {
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

    fn remove_copy(&self, fork: &ExternalId) -> bool {
        let Some(dir) = self.session_dir.as_deref() else {
            return false;
        };
        // Do not use an existence probe: it would mistake inaccessible
        // metadata for a copy that is already gone.
        let file = dir.join(format!("{}.jsonl", fork.as_str()));
        match self.remove_file(&file) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        }
    }
}

/// Only a saved copy can be reopened, so a bare session id does not count.
fn fork_key(state: &Value) -> Option<String> {
    session_file_key(state.get("sessionFile")?.as_str()?).map(str::to_owned)
}
