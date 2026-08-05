use std::{future::Future, pin::Pin};

use crate::acp::{
    contract::ProjectionRevision, lifecycle::ServerRequestState,
    runtime::permissions::PermissionIntent, store::AdapterTurnCursor,
};

use super::{DurableAdapterEvent, PumpIntent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JournalReceipt {
    pub projection_revision: ProjectionRevision,
    turn_cursor: AdapterTurnCursor,
}

impl JournalReceipt {
    pub(in crate::acp::runtime) fn committed(
        projection_revision: ProjectionRevision,
        turn_cursor: AdapterTurnCursor,
    ) -> Self {
        Self {
            projection_revision,
            turn_cursor,
        }
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

    fn try_rebind(self, _intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)>
    where
        Self: Sized,
    {
        Err((self, PumpJournalError::Rejected))
    }
}
