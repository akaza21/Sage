use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterizationObservationEvent {
    InitializeWriteStarted(u64),
    ReadyCommitted,
    PromptWriteCompleted(u64),
    FirstNormalizedItemAccepted,
    CancelReceived(u64),
    CancelWriteCompleted(u64),
    CancelTerminalCommitted,
    CloseReceived,
    JobZero,
    CloseCommitted,
    Invalid,
}

pub(crate) trait CharacterizationObservationSink: Send + Sync {
    fn record(&self, event: CharacterizationObservationEvent);
}

#[derive(Clone)]
pub(crate) struct CharacterizationObservation {
    sink: Arc<dyn CharacterizationObservationSink>,
}

impl CharacterizationObservation {
    pub(crate) fn new(sink: Arc<dyn CharacterizationObservationSink>) -> Self {
        Self { sink }
    }

    pub(crate) fn record(&self, event: CharacterizationObservationEvent) {
        self.sink.record(event);
    }
}

impl std::fmt::Debug for CharacterizationObservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CharacterizationObservation")
            .finish_non_exhaustive()
    }
}
