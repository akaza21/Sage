use serde::Deserialize;

const MAX_PACKET_BYTES: usize = 512;
const FOCUS_OUTCOMES: [&str; 3] = [
    "exact-sage-id",
    "activity-summary-fallback",
    "no-restoration-when-no-valid-envelope",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RendererRecreationFrontendPacket {
    schema_version: u8,
    live_regions: LiveRegions,
    pagehide_listeners: u8,
    focus_outcome: String,
    projection_slots: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveRegions {
    polite: u8,
    assertive: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererRecreationPacketAdmitError {
    Oversize,
    InvalidJson,
    InvalidSchema,
    InvalidLiveRegions,
    InvalidPagehide,
    InvalidProjectionSlots,
    InvalidFocusOutcome,
}

pub(crate) fn admit_renderer_recreation_frontend_packet(
    raw: &str,
) -> Result<RendererRecreationFrontendPacket, RendererRecreationPacketAdmitError> {
    if raw.len() > MAX_PACKET_BYTES {
        return Err(RendererRecreationPacketAdmitError::Oversize);
    }
    let packet: RendererRecreationFrontendPacket =
        serde_json::from_str(raw).map_err(|_| RendererRecreationPacketAdmitError::InvalidJson)?;
    if packet.schema_version != 1 {
        return Err(RendererRecreationPacketAdmitError::InvalidSchema);
    }
    if packet.focus_outcome.len() > MAX_PACKET_BYTES {
        return Err(RendererRecreationPacketAdmitError::Oversize);
    }
    if !FOCUS_OUTCOMES.contains(&packet.focus_outcome.as_str()) {
        return Err(RendererRecreationPacketAdmitError::InvalidFocusOutcome);
    }
    let paired = (packet.live_regions.polite == 1 && packet.live_regions.assertive == 1)
        || (packet.live_regions.polite == 0 && packet.live_regions.assertive == 0);
    if !paired {
        return Err(RendererRecreationPacketAdmitError::InvalidLiveRegions);
    }
    if packet.pagehide_listeners > 1 {
        return Err(RendererRecreationPacketAdmitError::InvalidPagehide);
    }
    if packet.live_regions.polite == 1 && packet.pagehide_listeners != 1 {
        return Err(RendererRecreationPacketAdmitError::InvalidPagehide);
    }
    if packet.live_regions.polite == 0 && packet.pagehide_listeners != 0 {
        return Err(RendererRecreationPacketAdmitError::InvalidPagehide);
    }
    if packet.projection_slots > 2 {
        return Err(RendererRecreationPacketAdmitError::InvalidProjectionSlots);
    }
    Ok(packet)
}

impl RendererRecreationFrontendPacket {
    pub(crate) fn is_announcement_pair(&self) -> bool {
        self.live_regions.polite == 1
            && self.live_regions.assertive == 1
            && self.pagehide_listeners == 1
    }

    pub(crate) fn projection_slots(&self) -> u8 {
        self.projection_slots
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FOCUS_OUTCOMES, RendererRecreationPacketAdmitError,
        admit_renderer_recreation_frontend_packet,
    };

    fn packet(focus_outcome: &str, slots: u8) -> String {
        format!(
            "{{\"schemaVersion\":1,\"liveRegions\":{{\"polite\":1,\"assertive\":1}},\"pagehideListeners\":1,\"focusOutcome\":\"{focus_outcome}\",\"projectionSlots\":{slots}}}"
        )
    }

    #[test]
    fn admits_the_frozen_focus_outcome_union() {
        let policy: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/work-items/ADE-0006A-acp-session-foundation/contracts/evidence-policy-v1.json"
        ))
        .unwrap();
        let contracted = policy["rendererRecreationObservation"]["focusOutcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(contracted, FOCUS_OUTCOMES);
        for outcome in FOCUS_OUTCOMES {
            admit_renderer_recreation_frontend_packet(&packet(outcome, 2)).unwrap();
        }
        admit_renderer_recreation_frontend_packet(&packet("exact-sage-id", 1)).unwrap();
    }

    #[test]
    fn rejects_unknown_outcomes_extra_listeners_and_oversize_strings() {
        assert_eq!(
            admit_renderer_recreation_frontend_packet(&packet("not-a-focus-outcome", 1)),
            Err(RendererRecreationPacketAdmitError::InvalidFocusOutcome)
        );
        assert_eq!(
            admit_renderer_recreation_frontend_packet(
                "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":1,\"assertive\":1},\"pagehideListeners\":2,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":1}"
            ),
            Err(RendererRecreationPacketAdmitError::InvalidPagehide)
        );
        assert_eq!(
            admit_renderer_recreation_frontend_packet(
                "{\"schemaVersion\":1,\"liveRegions\":{\"polite\":2,\"assertive\":2},\"pagehideListeners\":1,\"focusOutcome\":\"exact-sage-id\",\"projectionSlots\":1}"
            ),
            Err(RendererRecreationPacketAdmitError::InvalidLiveRegions)
        );
        let oversize = format!(
            "{}\"padding\":\"{}\"}}",
            packet("exact-sage-id", 1).trim_end_matches('}'),
            "x".repeat(512)
        );
        assert_eq!(
            admit_renderer_recreation_frontend_packet(&oversize),
            Err(RendererRecreationPacketAdmitError::Oversize)
        );
    }
}
