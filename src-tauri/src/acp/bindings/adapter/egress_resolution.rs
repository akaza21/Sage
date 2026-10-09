use super::EgressResolution;
use crate::acp::transport::FrameWriteOutcome;

impl From<FrameWriteOutcome> for EgressResolution {
    fn from(outcome: FrameWriteOutcome) -> Self {
        match outcome {
            FrameWriteOutcome::Complete => Self::Written,
            FrameWriteOutcome::ZeroByteFailure => Self::WriteZeroBytes,
            FrameWriteOutcome::DeliveryUnknown { .. } => Self::DeliveryUnknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_write_outcomes_preserve_egress_delivery_resolution() {
        for (outcome, expected) in [
            (FrameWriteOutcome::Complete, EgressResolution::Written),
            (
                FrameWriteOutcome::ZeroByteFailure,
                EgressResolution::WriteZeroBytes,
            ),
            (
                FrameWriteOutcome::DeliveryUnknown { bytes_written: 0 },
                EgressResolution::DeliveryUnknown,
            ),
            (
                FrameWriteOutcome::DeliveryUnknown { bytes_written: 17 },
                EgressResolution::DeliveryUnknown,
            ),
        ] {
            assert_eq!(EgressResolution::from(outcome), expected);
        }
    }
}
