//! Messages sent while Claude is working, on the runtime side: each is a
//! request Sage waits on until Claude Code reports it queued (see
//! `bridge::steer`).

use super::{SdkBridge, Sent, sdk_frame};
use crate::acp::bindings::adapter::{
    AdapterEgress, AdapterEvent, AdapterRuntimeError, EgressResolution, EgressToken,
};

/// Request ids for mid-turn messages, apart from the session's own.
const STEER_REQUESTS: u64 = 1 << 62;

/// Mid-turn messages awaiting Claude Code's word, by id, with the request
/// id Sage waits on.
pub(super) struct Steering {
    open: Vec<(String, u64)>,
    next: u64,
}

impl Default for Steering {
    fn default() -> Self {
        Self {
            open: Vec::new(),
            next: STEER_REQUESTS,
        }
    }
}

impl Steering {
    /// The message as Claude Code's own queued input; the session's ACP
    /// state never sees it.
    pub(super) fn egress(
        &mut self,
        bridge: &mut SdkBridge,
        prompt: &crate::acp::intent::Prompt,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let text = prompt.resources.inline(prompt.as_str());
        let (sdk, uuid) = bridge
            .steer(&text)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        let frame = match sdk_frame(&sdk) {
            Ok(frame) => frame,
            Err(error) => {
                bridge.undo(&Sent::Steer(uuid));
                return Err(error);
            }
        };
        let request_id = self.next;
        self.next += 1;
        self.open.push((uuid, request_id));
        Ok(AdapterEgress::new(frame, EgressToken::request(request_id)))
    }

    /// Each message's answer, once.
    pub(super) fn answers(&mut self, steers: Vec<(String, bool)>) -> Vec<AdapterEvent> {
        steers
            .into_iter()
            .filter_map(|(uuid, accepted)| {
                let index = self.open.iter().position(|(open, _)| *open == uuid)?;
                let (_, request_id) = self.open.swap_remove(index);
                Some(AdapterEvent::SteerResponse {
                    request_id,
                    accepted,
                })
            })
            .collect()
    }

    /// Settles a message's delivery; false when the token is not one.
    pub(super) fn resolve(
        &mut self,
        token: &EgressToken,
        resolution: EgressResolution,
        bridge: Option<&mut SdkBridge>,
    ) -> bool {
        let Some(index) = token
            .request_id()
            .and_then(|id| self.open.iter().position(|(_, open)| *open == id))
        else {
            return false;
        };
        if matches!(
            resolution,
            EgressResolution::QueueRejected | EgressResolution::WriteZeroBytes
        ) {
            let (uuid, _) = self.open.swap_remove(index);
            if let Some(bridge) = bridge {
                bridge.undo(&Sent::Steer(uuid));
            }
        }
        true
    }
}
