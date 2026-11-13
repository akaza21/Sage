use super::{CodexAdapter, LIMITS};

impl CodexAdapter {
    /// Posts at most once per review ID in the active or just-finished Turn.
    /// Each ledger retains at most `items_per_turn` IDs, each already checked
    /// to be at most 512 bytes. At capacity, suppress new notices rather than
    /// evict an ID and let a replay post again. The ledger moves to the Turn
    /// tombstone on completion and retires with it, leaving the next Turn
    /// its own empty ledger and full capacity.
    pub(crate) fn complete_approval_review(&mut self, turn: &str, review: &str) -> bool {
        let reviews = if self
            .active_turn_id
            .as_ref()
            .is_some_and(|active| active.as_str() == turn)
        {
            &mut self.active_approval_reviews
        } else if let Some(previous) = self
            .previous_turn
            .as_mut()
            .filter(|previous| previous.id.as_str() == turn)
        {
            &mut previous.approval_reviews
        } else {
            return false;
        };
        if reviews.len() >= LIMITS.wire.items_per_turn as usize
            || reviews.iter().any(|known| known == review)
        {
            return false;
        }
        reviews.push(review.to_owned());
        true
    }
}
