use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CharacterizationObservationEvent {
    #[cfg(windows)]
    InitializeWriteStarted(u64),
    ReadyCommitted,
    #[cfg(windows)]
    PromptWriteCompleted(u64),
    FirstNormalizedItemAccepted,
    #[cfg(windows)]
    CancelReceived(u64),
    #[cfg(windows)]
    CancelWriteCompleted(u64),
    CancelTerminalCommitted,
    #[cfg(windows)]
    CloseReceived,
    #[cfg(windows)]
    JobZero,
    CloseCommitted,
    #[cfg(windows)]
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
    #[cfg(windows)]
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
