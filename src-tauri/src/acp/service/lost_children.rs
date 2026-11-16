use super::AcpService;
use crate::acp::{
    projection::AcpChatPage,
    store::{StoreError, child_transcript::settle_lost_children},
};

impl AcpService {
    /// Settles the children an ended turn's row still reports live that no
    /// runtime held in this process can still run.
    pub(super) fn compose_lost_children(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        settle_lost_children(page, |owner| {
            self.lane_for(owner)
                .is_some_and(|lane| lane.runtime.for_owner(owner).is_ok())
        })
    }
}
