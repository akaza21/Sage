use crate::acp::{contract::RequiredText, goal::NativeRead};

use super::{AdapterError, CodexAdapter};
use crate::acp::bindings::codex::request::OutgoingRequest;

/// Sessions a runtime remembers from its listings, so an import reads only
/// a session of its own project that it listed. The import dialog shows at
/// most this many (`LISTED_SESSIONS_MAX` in `nativeSessionImport.ts`), so
/// every session it shows stays importable.
const LISTED_SESSIONS_MAX: usize = 256;

/// Abandoned import reads whose late answers are still dropped; past these,
/// a new read waits for Codex to answer one of them.
const MAX_ABANDONED_READS: usize = 4;

impl CodexAdapter {
    #[cfg(test)]
    pub(crate) fn learn_child_for_test(&mut self, child: &str) {
        self.child_threads
            .push(crate::acp::contract::SafeText::new("child", child).unwrap());
    }
    pub(crate) fn child_read_pending(&self) -> bool {
        self.pending_read.is_some()
    }

    /// A native read (awaited or abandoned) or a fork carries native Turns,
    /// and a skills read a whole catalogue; each may be answered past the
    /// frame bound.
    pub(crate) fn oversize_recoverable(&self) -> bool {
        self.child_read_pending()
            || !self.abandoned_reads.is_empty()
            || self.fork_pending().is_some()
            || self.skills_read_pending()
            || self.mcp_list_awaited()
    }

    pub(crate) fn read_scope(&self) -> Option<(u64, &NativeRead)> {
        self.pending_read.as_ref().map(|(id, read)| (*id, read))
    }

    /// One page of a native read. A child's Turns need a child this
    /// runtime spawned; an import needs `nativeSessionImport`, and its Turns
    /// a session this runtime listed.
    pub(crate) fn native_read(
        &mut self,
        id: u64,
        read: &NativeRead,
    ) -> Result<OutgoingRequest, AdapterError> {
        if !self.goal_supported()
            || self.pending_read.is_some()
            || self.abandoned_reads.len() >= MAX_ABANDONED_READS
        {
            return Err(AdapterError::CapabilityUnavailable);
        }
        let cursor = read.cursor();
        let request = match read {
            NativeRead::ChildTurns {
                child_thread_id: child,
                parent_thread_id: parent,
                ..
            } => {
                if self.external_thread_id.as_ref().map(|value| value.as_str())
                    != Some(parent.as_str())
                    || !self.is_child_thread(child)
                    || child == parent
                {
                    return Err(AdapterError::IdentityConflict);
                }
                let child = RequiredText::<512>::new("childThreadId", child.clone())?;
                OutgoingRequest::turns_list(id, &child, cursor, false)?
            }
            NativeRead::ImportableSessions { excluded, .. } => {
                self.require_method(|methods| methods.snapshot.native_session_import)?;
                let root = self
                    .native_root
                    .as_ref()
                    .ok_or(AdapterError::CapabilityUnavailable)?;
                OutgoingRequest::session_list(id, root, cursor, excluded)?
            }
            NativeRead::ImportTurns { thread_id, .. } => {
                self.require_method(|methods| methods.snapshot.native_session_import)?;
                if !self
                    .listed_sessions
                    .iter()
                    .any(|listed| listed == thread_id)
                {
                    return Err(AdapterError::IdentityConflict);
                }
                let thread = RequiredText::<512>::new("threadId", thread_id.clone())?;
                OutgoingRequest::turns_list(id, &thread, cursor, true)?
            }
        };
        let request = self.reserve(request)?;
        self.pending_read = Some((id, read.clone()));
        Ok(request)
    }

    /// Remembers the sessions a listing returned as the most recently
    /// listed, dropping the least recently listed past the bound: a session
    /// listed again stays importable as long as one listed for the first
    /// time.
    pub(crate) fn remember_listed(&mut self, threads: impl IntoIterator<Item = String>) {
        for thread in threads {
            self.listed_sessions.retain(|listed| *listed != thread);
            self.listed_sessions.push(thread);
        }
        let excess = self
            .listed_sessions
            .len()
            .saturating_sub(LISTED_SESSIONS_MAX);
        self.listed_sessions.drain(..excess);
    }

    pub(crate) fn complete_native_read(&mut self, id: u64) -> Result<(), AdapterError> {
        if self.read_scope().map(|(pending, _)| pending) != Some(id)
            || !self
                .pending_kind(id)
                .is_some_and(|kind| kind.is_native_read())
        {
            return Err(AdapterError::ForeignRequest);
        }
        self.pending_read = None;
        let removed = self.take_pending(id);
        debug_assert!(removed.is_some_and(|kind| kind.is_native_read()));
        self.child_read_tombstone = Some(id);
        Ok(())
    }

    /// A native read's answer past the frame bound: `true` when the read
    /// that awaited it is over, too large to show (a child transcript, an
    /// imported session's Turns or an MCP server list).
    pub(crate) fn oversized_child_response(&mut self, id: u64) -> Result<bool, AdapterError> {
        if self.mcp_list_oversized(id) {
            return Ok(true);
        }
        if self.take_abandoned_read(id) {
            return Ok(false);
        }
        if self.read_scope().map(|(pending, _)| pending) == Some(id) {
            self.complete_native_read(id)?;
            return Ok(true);
        }
        if self.pending_kind(id).is_some() {
            return Err(AdapterError::ForeignRequest);
        }
        Ok(false)
    }

    pub(crate) fn is_child_read_tombstone(&self, id: u64) -> bool {
        self.child_read_tombstone == Some(id)
    }

    pub(crate) fn is_child_read_tombstone_id_active(&self) -> bool {
        self.child_read_tombstone.is_some() || !self.abandoned_reads.is_empty()
    }

    /// Stops waiting for a read whose caller gave up, when the read abandons
    /// at its deadline; its late answer is dropped by `take_abandoned_read`.
    pub(crate) fn abandon_native_read(&mut self) {
        let Some(id) = self
            .read_scope()
            .filter(|(_, read)| read.abandons_at_deadline())
            .map(|(id, _)| id)
        else {
            return;
        };
        self.pending_read = None;
        let removed = self.take_pending(id);
        debug_assert!(removed.is_some_and(|kind| kind.is_native_read()));
        self.abandoned_reads.push(id);
    }

    /// Whether `id` answers an abandoned read, which is then over.
    pub(crate) fn take_abandoned_read(&mut self, id: u64) -> bool {
        let before = self.abandoned_reads.len();
        self.abandoned_reads.retain(|dead| *dead != id);
        self.abandoned_reads.len() != before
    }
}
