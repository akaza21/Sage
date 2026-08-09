use crate::acp::{
    characterization_observation::{CharacterizationObservation, CharacterizationObservationEvent},
    process::{StdinEndpoint, monotonic_tick},
    runtime::{ActivationTransport, EgressTransport, StdinTransport},
    transport::{FrameWriteOutcome, OutboundFrame},
};

use super::types::CharacterizationCase;

pub(super) struct ObservedStdin {
    inner: StdinEndpoint,
    observation: CharacterizationObservation,
    case: CharacterizationCase,
    successful_writes: u8,
    closed: bool,
}

impl ObservedStdin {
    pub(super) fn new(
        inner: StdinEndpoint,
        observation: CharacterizationObservation,
        case: CharacterizationCase,
    ) -> Self {
        Self {
            inner,
            observation,
            case,
            successful_writes: 0,
            closed: false,
        }
    }

    fn write_observed(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        if self.closed {
            self.invalid();
            return FrameWriteOutcome::ZeroByteFailure;
        }
        let started = monotonic_tick().ok();
        let outcome = self.inner.write_frame(frame);
        match outcome {
            FrameWriteOutcome::Complete => {
                self.successful_writes = self.successful_writes.saturating_add(1);
                match self.successful_writes {
                    1 => match started {
                        Some(tick) => self.observation.record(
                            CharacterizationObservationEvent::InitializeWriteStarted(tick),
                        ),
                        None => self.invalid(),
                    },
                    2 => {}
                    3 => self
                        .record_completed(CharacterizationObservationEvent::PromptWriteCompleted),
                    4 if self.case != CharacterizationCase::TurnSuccess => self
                        .record_completed(CharacterizationObservationEvent::CancelWriteCompleted),
                    _ => self.invalid(),
                }
            }
            FrameWriteOutcome::ZeroByteFailure => {}
            FrameWriteOutcome::DeliveryUnknown { .. } => self.invalid(),
        }
        outcome
    }

    fn record_completed(&self, event: fn(u64) -> CharacterizationObservationEvent) {
        match monotonic_tick() {
            Ok(tick) => self.observation.record(event(tick)),
            Err(_) => self.invalid(),
        }
    }

    fn invalid(&self) {
        self.observation
            .record(CharacterizationObservationEvent::Invalid);
    }
}

impl ActivationTransport for ObservedStdin {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.write_observed(frame)
    }
}

impl EgressTransport for ObservedStdin {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.write_observed(frame)
    }
}

impl StdinTransport for ObservedStdin {
    fn close(&mut self) {
        if !self.closed {
            let expected = if self.case == CharacterizationCase::TurnSuccess {
                3
            } else {
                4
            };
            if self.successful_writes != expected {
                self.invalid();
            }
            self.closed = true;
        }
        self.inner.close();
    }
}
