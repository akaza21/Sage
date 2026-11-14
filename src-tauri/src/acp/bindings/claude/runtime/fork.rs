use super::*;
use crate::acp::goal::NativeRpcReply;

impl ClaudeRuntime {
    /// No `_meta` fork point is sent, so the adapter forks through the latest message.
    pub(super) fn fork_command(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.require_capability(|value| value.native_session_fork)?;
        if self.fork_pending() {
            return Err(AdapterRuntimeError::Operation);
        }
        // A wanted delete is never evicted: with every slot wanted, a further
        // fork is a capacity refusal until the deletes are written at idle.
        self.evict_settled_forks(MAX_TRACKED_FORKS + 1);
        if self.forks.len() >= MAX_TRACKED_FORKS && self.forks.iter().all(|fork| fork.wanted) {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let active = self.active.as_ref().ok_or(AdapterRuntimeError::Operation)?;
        if active.turn.is_some() {
            return Err(AdapterRuntimeError::Operation);
        }
        let source = active.external_id.clone();
        let root = self
            .session_root
            .clone()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.native_rpc_reply = None;
        self.reserve(
            PendingRequest::Fork(source.clone()),
            "session/fork",
            json!({"sessionId": source.as_str(), "cwd": root}),
        )
    }

    pub(super) fn fork_pending(&self) -> bool {
        self.pending.values().any(|pending| {
            matches!(
                pending,
                PendingRequest::Fork(_) | PendingRequest::DeleteFork(false, _)
            )
        })
    }

    /// The new session is not bound here: the fork Chat resumes it in its own runtime. A ledger
    /// with no settled fork answers `Rejected`, but the adapter has already stored the copy, so
    /// it is tracked with its delete wanted.
    pub(super) fn fork_response(
        &mut self,
        value: &Value,
        source: &ExternalId,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut reply = fork_reply(value, source);
        if let NativeRpcReply::Forked(fork) = &reply
            && !self.is_fork(fork.as_str())
        {
            self.evict_settled_forks(MAX_TRACKED_FORKS);
            let wanted = self.forks.len() >= MAX_TRACKED_FORKS;
            self.forks.push(TrackedFork {
                id: fork.clone(),
                wanted,
                failures: 0,
            });
            if wanted {
                reply = NativeRpcReply::Rejected;
            }
        }
        self.native_rpc_reply = Some(reply);
        AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new())
    }

    fn evict_settled_forks(&mut self, limit: usize) {
        while self.forks.len() >= limit {
            let Some(settled) = self.forks.iter().position(|fork| !fork.wanted) else {
                return;
            };
            self.forks.remove(settled);
        }
    }

    /// claude-agent-acp 0.76.0 forks storage only (`fork-session.js`); the guard keeps a
    /// later adapter that announces the fork from tearing the parent down.
    pub(super) fn absorbs_fork_update(&self, value: &Value) -> bool {
        let Some(session) = value.pointer("/params/sessionId").and_then(Value::as_str) else {
            return false;
        };
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.external_id.as_str() == session)
        {
            return false;
        }
        self.fork_pending() || self.is_fork(session)
    }

    fn is_fork(&self, session: &str) -> bool {
        self.forks.iter().any(|fork| fork.id.as_str() == session)
    }

    pub(super) fn want_fork_delete(&mut self, fork: &ExternalId) {
        if let Some(entry) = self.forks.iter_mut().find(|known| known.id == *fork) {
            entry.failures = entry.failures.saturating_add(1);
            entry.wanted = entry.failures < MAX_FORK_DELETE_ATTEMPTS;
        }
    }

    pub(super) fn defer_fork_release_command(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> bool {
        let Some(entry) = self.forks.iter_mut().find(|known| known.id == *fork) else {
            return false;
        };
        entry.wanted |= !committed;
        self.native_rpc_reply = Some(NativeRpcReply::Released);
        true
    }

    pub(super) fn deferred_fork_delete(
        &mut self,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        let idle = self
            .active
            .as_ref()
            .is_some_and(|active| active.turn.is_none());
        let busy = self
            .pending
            .values()
            .any(|pending| matches!(pending, PendingRequest::DeleteFork(..)));
        let Some(entry) = self.forks.iter_mut().find(|fork| fork.wanted) else {
            return Ok(None);
        };
        if !idle || busy {
            return Ok(None);
        }
        entry.wanted = false;
        let fork = entry.id.clone();
        self.reserve(
            PendingRequest::DeleteFork(true, fork.clone()),
            "session/delete",
            json!({"sessionId": fork.as_str()}),
        )
        .map(Some)
    }

    /// The 0.76.0 fork is a stored copy with no live state, so a committed
    /// fork holds nothing to close (`session/close` answers "Session not
    /// found", see the captured trace). A refused fork's orphaned copy is
    /// removed with `session/delete`, which the adapter advertises.
    pub(super) fn release_fork_command(
        &mut self,
        fork: &ExternalId,
        committed: bool,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        if !self.is_fork(fork.as_str()) || self.fork_pending() {
            return Err(AdapterRuntimeError::Operation);
        }
        if committed {
            self.native_rpc_reply = Some(NativeRpcReply::Released);
            return Ok(None);
        }
        self.native_rpc_reply = None;
        self.reserve(
            PendingRequest::DeleteFork(false, fork.clone()),
            "session/delete",
            json!({"sessionId": fork.as_str()}),
        )
        .map(Some)
    }

    pub(super) fn delete_fork_response(
        &mut self,
        value: &Value,
        background: bool,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if !background {
            self.native_rpc_reply = Some(
                if value.get("error").is_none() && value.get("result").is_some_and(Value::is_object)
                {
                    NativeRpcReply::Released
                } else {
                    NativeRpcReply::Rejected
                },
            );
        }
        AdapterBatch::new(vec![SharedEvent::IgnoredLate], Vec::new())
    }
}

/// Forks remembered per runtime so their late frames stay absorbed.
pub(super) const MAX_TRACKED_FORKS: usize = 8;

/// Unwritten deletes a fork survives before it is given up, as Codex does.
pub(super) const MAX_FORK_DELETE_ATTEMPTS: u8 = 3;

#[derive(Debug)]
pub(super) struct TrackedFork {
    id: ExternalId,
    wanted: bool,
    failures: u8,
}

pub(super) fn fork_reply(value: &Value, source: &ExternalId) -> NativeRpcReply {
    if value.get("error").is_some() {
        return NativeRpcReply::Rejected;
    }
    match value
        .get("result")
        .and_then(|result| result.get("sessionId"))
        .and_then(Value::as_str)
    {
        Some(id) if id != source.as_str() => ExternalId::new("externalSessionId", id.to_owned())
            .map_or(NativeRpcReply::Rejected, NativeRpcReply::Forked),
        _ => NativeRpcReply::Rejected,
    }
}
