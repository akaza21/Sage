use std::sync::Arc;

use super::{ReadState, ReadTextOwner, TerminalReceipt, state_machine};

/// An explicit receipt handle shares the coordinator state, so it remains
/// drainable after the coordinator is dropped without creating a second lock
/// or an independently counted occupancy domain.
#[allow(dead_code, reason = "P-02 explicit adapter receipt handle")]
#[derive(Clone)]
pub(crate) struct ReadTextReceiptSink(pub(super) Arc<ReadState>);

#[allow(dead_code, reason = "P-02 explicit adapter receipt handle")]
impl ReadTextReceiptSink {
    pub(crate) fn new(state: Arc<ReadState>) -> Self {
        Self(state)
    }

    pub(crate) fn drain_owner(&self, owner: &ReadTextOwner) -> Vec<TerminalReceipt> {
        state_machine::drain_receipts(&self.0, owner)
    }
}
