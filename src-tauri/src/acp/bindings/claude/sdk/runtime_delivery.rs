use super::ClaudeSdkRuntime;
use crate::acp::bindings::adapter::{EgressResolution, EgressToken};

impl ClaudeSdkRuntime {
    /// Tells the bridge whether a frame it started reached Claude Code, so a
    /// frame never received is undone there too.
    pub(super) fn settle_sent(&mut self, token: &EgressToken, resolution: EgressResolution) {
        if matches!(resolution, EgressResolution::Queued) {
            return;
        }
        let Some(index) = self.sent.iter().position(|(sent, _)| sent == token) else {
            return;
        };
        let (_, sent) = self.sent.swap_remove(index);
        let Some(bridge) = self.bridge.as_mut() else {
            return;
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
        ) {
            bridge.undo(&sent);
        } else {
            bridge.delivered(&sent);
        }
    }
}
