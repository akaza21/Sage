use super::{ReadTextFileService, ReadTextOwner, terminal};

impl ReadTextFileService {
    /// Revokes every token-keyed attempt, including source I/O and already
    /// claimed completion leases. Revocation is first-terminal-wins.
    pub(crate) fn terminalize_all(&self) -> terminal::ReadTextSettlementHandle {
        let tokens = terminal::terminalize_all(&self.state, None);
        terminal::settlement_handle(std::sync::Arc::clone(&self.state), tokens)
    }
    #[allow(
        dead_code,
        reason = "P-02 Decision B deferred adapter seam; exercised by conformance tests and not production-admitted"
    )]
    pub(crate) fn terminalize_owner(
        &self,
        owner: &ReadTextOwner,
    ) -> terminal::ReadTextSettlementHandle {
        let tokens = terminal::terminalize_all(&self.state, Some(owner));
        terminal::settlement_handle(std::sync::Arc::clone(&self.state), tokens)
    }
}
impl Drop for ReadTextFileService {
    fn drop(&mut self) {
        // Drop cannot report failure, but it must apply the same bounded
        // cancellation/settlement policy as service shutdown. If this expires
        // the Arc-held writer state remains live and no completed teardown is
        // claimed while it could still emit a frame.
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.ordinary_rpc);
        let settlement = self.terminalize_all();
        let _ = settlement.wait_until(deadline);
    }
}
