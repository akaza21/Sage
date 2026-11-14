use std::{future::Future, pin::Pin};

use crate::{
    acp::{contract::ProjectionRevision, lifecycle::ServerRequestState, store::AdapterTurnCursor},
    acp_protocol::permissions::PermissionIntent,
};

use super::{DurableAdapterEvent, PumpIntent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalReceipt {
    pub projection_revision: ProjectionRevision,
    turn_cursor: AdapterTurnCursor,
    /// Set when the batch advanced the chat's child projection epoch.
    pub child_projection_epoch: Option<u64>,
    /// False when every event in the batch was already durable.
    applied: bool,
}

impl JournalReceipt {
    pub(in crate::acp::runtime) fn committed(
        projection_revision: ProjectionRevision,
        turn_cursor: AdapterTurnCursor,
    ) -> Self {
        Self {
            projection_revision,
            turn_cursor,
            child_projection_epoch: None,
            applied: true,
        }
    }

    pub(in crate::acp::runtime) fn with_applied(mut self, applied: bool) -> Self {
        self.applied = applied;
        self
    }

    pub(crate) fn applied(&self) -> bool {
        self.applied
    }

    pub(in crate::acp::runtime) fn with_child_projection_epoch(
        mut self,
        epoch: Option<u64>,
    ) -> Self {
        self.child_projection_epoch = epoch;
        self
    }

    pub(crate) fn turn_cursor(&self) -> &AdapterTurnCursor {
        &self.turn_cursor
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PumpJournalError {
    #[error("durable adapter batch was rejected")]
    Rejected,
    #[error("durable adapter commit outcome is unknown")]
    CommitUnknown,
}

pub(crate) trait AdapterBatchJournal: Send {
    fn commit<'a>(
        &'a mut self,
        events: Vec<DurableAdapterEvent>,
        actual_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Result<JournalReceipt, PumpJournalError>> + Send + 'a>>;

    fn transition_permission<'a>(
        &'a mut self,
        _intent: &'a PermissionIntent,
        _state: ServerRequestState,
        _now_millis: i64,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectionRevision, PumpJournalError>> + Send + 'a>>
    {
        Box::pin(async { Err(PumpJournalError::Rejected) })
    }

    #[cfg(feature = "acp-fixture")]
    fn first_normalized_item_accepted(&mut self) {}

    fn try_rebind(self, _intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)>
    where
        Self: Sized,
    {
        Err((self, PumpJournalError::Rejected))
    }
}
