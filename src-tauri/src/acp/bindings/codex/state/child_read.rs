use crate::acp::contract::RequiredText;

use super::{AdapterError, CodexAdapter, RequestKind};
use crate::acp::bindings::codex::request::OutgoingRequest;

impl CodexAdapter {
    #[cfg(test)]
    pub(crate) fn learn_child_for_test(&mut self, child: &str) {
        self.child_threads
            .push(crate::acp::contract::SafeText::new("child", child).unwrap());
    }
    pub(crate) fn child_read_pending(&self) -> bool {
        self.pending_child_read.is_some()
    }

    pub(crate) fn child_read_scope(&self) -> Option<(u64, &str, &str)> {
        self.pending_child_read
            .as_ref()
            .map(|(id, child, parent)| (*id, child.as_str(), parent.as_str()))
    }

    pub(crate) fn child_read(
        &mut self,
        id: u64,
        child: &str,
        parent: &str,
    ) -> Result<OutgoingRequest, AdapterError> {
        if !self.goal_supported() || self.pending_child_read.is_some() {
            return Err(AdapterError::CapabilityUnavailable);
        }
        if self.external_thread_id.as_ref().map(|value| value.as_str()) != Some(parent)
            || !self.is_child_thread(child)
            || child == parent
        {
            return Err(AdapterError::IdentityConflict);
        }
        let child_id = RequiredText::<512>::new("childThreadId", child.to_owned())?;
        let request = self.reserve(OutgoingRequest::child_read(id, &child_id)?)?;
        self.pending_child_read = Some((id, child.to_owned(), parent.to_owned()));
        Ok(request)
    }

    pub(crate) fn complete_child_read(&mut self, id: u64) -> Result<(), AdapterError> {
        if self.child_read_scope().map(|(pending, _, _)| pending) != Some(id)
            || self.pending_kind(id) != Some(RequestKind::ChildRead)
        {
            return Err(AdapterError::ForeignRequest);
        }
        self.pending_child_read = None;
        let removed = self.take_pending(id);
        debug_assert_eq!(removed, Some(RequestKind::ChildRead));
        self.child_read_tombstone = Some(id);
        Ok(())
    }

    pub(crate) fn oversized_child_response(&mut self, id: u64) -> Result<bool, AdapterError> {
        if self.child_read_scope().map(|(pending, _, _)| pending) == Some(id) {
            self.complete_child_read(id)?;
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
