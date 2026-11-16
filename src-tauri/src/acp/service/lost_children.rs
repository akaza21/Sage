use super::AcpService;
use crate::acp::{
    projection::AcpChatPage,
    store::{StoreError, child_transcript::settle_lost_children},
};

impl AcpService {
    /// Settles the children an ended turn's row still reports live that no
    /// runtime held in this process can still run. A recovering runtime still
    /// holds its owner; one released after a failed recovery does not, and
    /// that recovery durably ends the epoch, which publishes the change.
    pub(super) fn compose_lost_children(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        settle_lost_children(page, |owner| {
            self.lane_for(owner)
                .is_some_and(|lane| lane.runtime.runs(owner))
        })
    }
}
