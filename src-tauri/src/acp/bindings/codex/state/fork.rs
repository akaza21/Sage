use serde_json::Value;

use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::{
    bindings::codex::request::OutgoingRequest, goal::NativeRpcReply, intent::ExternalId,
};

/// Codex loads a fork into the app-server that served `thread/fork` and
/// announces it there (`thread/started`, MCP startup status), so the parent
/// runtime must absorb that thread's notifications without binding it, then
/// release it once the fork Chat is committed (`thread/unsubscribe`) or refused
/// (`thread/delete`, which also removes the orphan rollout). The cap bounds how
/// many fork threads one parent runtime can keep loaded.
pub(crate) const MAX_UNRELEASED_FORKS: usize = 8;

/// Codex refusals (or unwritten frames) a fork release survives before the
/// ledger gives the thread up, so a slot never stays held by a release that
/// can never land. Each failed attempt is retried at the next idle moment.
pub(crate) const MAX_FORK_RELEASE_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForkState {
    Loaded,
    /// A release waits for the next idle moment; `delete` for a refused fork.
    Wanted {
        delete: bool,
    },
    /// Released, or given up after `MAX_FORK_RELEASE_ATTEMPTS`; its late
    /// notifications stay absorbed either way.
    Released,
}

#[derive(Clone, Debug, PartialEq)]
struct ForkEntry {
    thread: ExternalId,
    state: ForkState,
    failures: u8,
}

/// A release in flight; a background one is a deferred release nobody waits for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingRelease {
    id: u64,
    index: usize,
    background: bool,
    delete: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ForkLedger {
    /// Recent fork threads, oldest first, each with its release state.
    threads: Vec<ForkEntry>,
    pending_release: Option<PendingRelease>,
}

impl ForkLedger {
    fn contains(&self, thread: &str) -> bool {
        self.threads
            .iter()
            .any(|entry| entry.thread.as_str() == thread)
    }

    fn unreleased(&self) -> usize {
        self.threads
            .iter()
            .filter(|entry| entry.state != ForkState::Released)
            .count()
    }

    fn position_unreleased(&self, thread: &str) -> Option<usize> {
        self.threads
            .iter()
            .position(|entry| entry.thread.as_str() == thread && entry.state != ForkState::Released)
    }

    fn next_wanted(&self, bound: Option<&str>) -> Option<(usize, bool)> {
        self.threads
            .iter()
            .enumerate()
            .find_map(|(index, entry)| match entry.state {
                ForkState::Wanted { delete } if Some(entry.thread.as_str()) != bound => {
                    Some((index, delete))
                }
                _ => None,
            })
    }

    fn remember(&mut self, thread: ExternalId) {
        if self.contains(thread.as_str()) {
            return;
        }
        if self.threads.len() == MAX_UNRELEASED_FORKS
            && let Some(oldest) = self
                .threads
                .iter()
                .position(|entry| entry.state == ForkState::Released)
        {
            self.threads.remove(oldest);
            if let Some(pending) = self.pending_release.as_mut()
                && pending.index > oldest
            {
                pending.index -= 1;
            }
        }
        if self.threads.len() < MAX_UNRELEASED_FORKS {
            self.threads.push(ForkEntry {
                thread,
                state: ForkState::Loaded,
                failures: 0,
            });
        }
    }

    fn release_failed(&mut self, index: usize, delete: bool) {
        let Some(entry) = self.threads.get_mut(index) else {
            return;
        };
        if entry.state == ForkState::Released {
            return;
        }
        entry.failures = entry.failures.saturating_add(1);
        entry.state = if entry.failures >= MAX_FORK_RELEASE_ATTEMPTS {
            ForkState::Released
        } else {
            ForkState::Wanted { delete }
        };
    }

    pub(super) fn release_unwritten(&mut self, id: u64) {
        if let Some(pending) = self.pending_release
            && pending.id == id
        {
            self.pending_release = None;
            self.release_failed(pending.index, pending.delete);
        }
    }
}

impl CodexAdapter {
    pub(crate) fn fork_pending(&self) -> Option<u64> {
        self.pending_fork
    }

    /// True for a thread this runtime forked, and for any foreign thread
    /// while a fork is in flight: Codex announces the fork thread before it
    /// answers `thread/fork` (see the captured `codex/fork-notifications`).
    pub(crate) fn is_fork_thread(&self, thread: &str) -> bool {
        self.pending_fork.is_some() || self.forks.contains(thread)
    }

    pub(crate) fn remember_fork(&mut self, thread: ExternalId) {
        self.forks.remember(thread);
    }

    /// A deferred release is background work and never holds the RPC slot.
    pub(crate) fn fork_rpc_pending(&self) -> bool {
        self.pending_fork.is_some()
            || self
                .forks
                .pending_release
                .is_some_and(|pending| !pending.background)
    }

    pub(crate) fn fork_release_pending(&self) -> Option<(u64, bool)> {
        self.forks
            .pending_release
            .map(|pending| (pending.id, pending.background))
    }

    pub(crate) fn fork_ledger_full(&self) -> bool {
        self.forks.unreleased() >= MAX_UNRELEASED_FORKS
    }

    /// A deferred release holds the release slot: a foreground release
    /// waits behind it for the next idle moment, exactly like one asked for
    /// during a Turn. False when no background release is in flight.
    pub(crate) fn queue_fork_release(&mut self, thread: &str, delete: bool) -> bool {
        if !self
            .forks
            .pending_release
            .is_some_and(|pending| pending.background)
            || self.bound_thread_id() == Some(thread)
        {
            return false;
        }
        let Some(index) = self.forks.position_unreleased(thread) else {
            return false;
        };
        self.forks.threads[index].state = ForkState::Wanted { delete };
        true
    }

    /// Reserves the release of an unreleased fork thread (`delete` for a
    /// refused fork). The bound thread itself is never released this way.
    pub(crate) fn release_fork(
        &mut self,
        id: u64,
        thread: &str,
        delete: bool,
    ) -> Result<OutgoingRequest, AdapterError> {
        let index = self
            .forks
            .position_unreleased(thread)
            .ok_or(AdapterError::ForeignRequest)?;
        if self.forks.pending_release.is_some() || self.bound_thread_id() == Some(thread) {
            return Err(AdapterError::ForeignRequest);
        }
        let request = self.reserve(OutgoingRequest::fork_release(id, thread, delete)?)?;
        self.forks.pending_release = Some(PendingRelease {
            id,
            index,
            background: false,
            delete,
        });
        Ok(request)
    }

    pub(crate) fn defer_fork_release(&mut self, thread: &str, delete: bool) -> bool {
        if self.bound_thread_id() == Some(thread) {
            return false;
        }
        let Some(entry) = self
            .forks
            .threads
            .iter_mut()
            .find(|entry| entry.thread.as_str() == thread && entry.state == ForkState::Loaded)
        else {
            return false;
        };
        entry.state = ForkState::Wanted { delete };
        true
    }

    pub(crate) fn has_deferred_fork_release(&self) -> bool {
        self.forks.pending_release.is_none()
            && self.active_turn_id.is_none()
            && self.forks.next_wanted(self.bound_thread_id()).is_some()
    }

    /// The bound thread is never released this way.
    pub(crate) fn next_deferred_fork_release(
        &mut self,
        id: u64,
    ) -> Result<OutgoingRequest, AdapterError> {
        if !self.has_deferred_fork_release() {
            return Err(AdapterError::ForeignRequest);
        }
        let (index, delete) = self
            .forks
            .next_wanted(self.bound_thread_id())
            .ok_or(AdapterError::ForeignRequest)?;
        let thread = self.forks.threads[index].thread.clone();
        let request = self.reserve(OutgoingRequest::fork_release(id, thread.as_str(), delete)?)?;
        self.forks.threads[index].state = ForkState::Loaded;
        self.forks.pending_release = Some(PendingRelease {
            id,
            index,
            background: true,
            delete,
        });
        Ok(request)
    }

    /// Settles the pending release; `released` is false when Codex refused
    /// it, which leaves it wanted for a bounded retry at the next idle
    /// moment. Returns whether it was a background (deferred) release.
    pub(crate) fn complete_fork_release(
        &mut self,
        id: u64,
        released: bool,
    ) -> Result<bool, AdapterError> {
        let Some(PendingRelease {
            id: pending,
            index,
            background,
            delete,
        }) = self.forks.pending_release
        else {
            return Err(AdapterError::ForeignRequest);
        };
        let kind = self.pending_kind(id);
        if pending != id
            || !matches!(
                kind,
                Some(RequestKind::ForkRelease | RequestKind::ForkDelete)
            )
        {
            return Err(AdapterError::ForeignRequest);
        }
        self.forks.pending_release = None;
        if released {
            if let Some(entry) = self.forks.threads.get_mut(index) {
                entry.state = ForkState::Released;
            }
        } else {
            self.forks.release_failed(index, delete);
        }
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, kind);
        Ok(background)
    }

    /// Only a characterised negotiation admits it, and only between Turns: the
    /// fork carries the thread through its latest completed Turn.
    pub(crate) fn fork(&mut self, id: u64) -> Result<OutgoingRequest, AdapterError> {
        self.require_method(|methods| methods.snapshot.native_session_fork)?;
        if self.pending_fork.is_some()
            || self.pending_read.is_some()
            || self.turn_start_correlations.has_pending()
            || self.active_turn_id.is_some()
            || self.provisional_turn.is_some()
        {
            return Err(AdapterError::TurnAlreadyActive);
        }
        if self.fork_ledger_full() {
            return Err(AdapterError::PendingFull);
        }
        let thread = self
            .external_thread_id
            .clone()
            .ok_or(AdapterError::SessionUnavailable)?;
        let request = self.reserve(OutgoingRequest::fork(id, &thread)?)?;
        self.pending_fork = Some(id);
        Ok(request)
    }

    pub(crate) fn complete_fork(&mut self, id: u64) -> Result<(), AdapterError> {
        if self.pending_fork != Some(id) || self.pending_kind(id) != Some(RequestKind::Fork) {
            return Err(AdapterError::ForeignRequest);
        }
        self.pending_fork = None;
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, Some(RequestKind::Fork));
        Ok(())
    }

    /// A non-null `forkedFromId` in the fork response must match this source.
    pub(crate) fn bound_thread_id(&self) -> Option<&str> {
        self.external_thread_id.as_ref().map(|value| value.as_str())
    }
}

/// Reads only `result.thread.id` (and `forkedFromId`, when present) from a
/// `thread/fork` response. The rest of the response (the forked Turns) is
/// native history Sage does not import, so it is not shape-checked here.
pub(crate) fn fork_reply(value: &Value, source: Option<&str>) -> NativeRpcReply {
    if value.get("error").is_some() {
        return NativeRpcReply::Rejected;
    }
    let Some(thread) = value
        .get("result")
        .and_then(|result| result.get("thread"))
        .and_then(Value::as_object)
    else {
        return NativeRpcReply::Rejected;
    };
    let forked_from = thread.get("forkedFromId");
    let lineage_matches = match (forked_from, source) {
        (None | Some(Value::Null), Some(_)) => true,
        (Some(Value::String(from)), Some(source)) => from == source,
        _ => false,
    };
    match thread.get("id").and_then(Value::as_str) {
        Some(id) if lineage_matches && Some(id) != source => {
            ExternalId::new("externalSessionId", id.to_owned())
                .map_or(NativeRpcReply::Rejected, NativeRpcReply::Forked)
        }
        _ => NativeRpcReply::Rejected,
    }
}
