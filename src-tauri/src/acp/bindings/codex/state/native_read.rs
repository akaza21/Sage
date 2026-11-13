use crate::acp::{contract::RequiredText, goal::NativeRead};

use super::{AdapterError, CodexAdapter};
use crate::acp::bindings::codex::request::OutgoingRequest;

/// Sessions a runtime remembers from its listings, so an import reads only
/// a session of its own project that it listed.
const LISTED_SESSIONS_MAX: usize = 256;

impl CodexAdapter {
    #[cfg(test)]
    pub(crate) fn learn_child_for_test(&mut self, child: &str) {
        self.child_threads
            .push(crate::acp::contract::SafeText::new("child", child).unwrap());
    }
    pub(crate) fn child_read_pending(&self) -> bool {
        self.pending_read.is_some()
    }

    /// A native read or a fork carries native Turns, and a skills read a
    /// whole catalogue; each may be answered past the frame bound.
    pub(crate) fn oversize_recoverable(&self) -> bool {
        self.child_read_pending()
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
        if !self.goal_supported() || self.pending_read.is_some() {
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

    /// Remembers the sessions a listing returned, dropping the oldest past
    /// the bound.
    pub(crate) fn remember_listed(&mut self, threads: impl IntoIterator<Item = String>) {
        for thread in threads {
            if !self.listed_sessions.contains(&thread) {
                self.listed_sessions.push(thread);
            }
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
        self.child_read_tombstone.is_some()
    }
}
