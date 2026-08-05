use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::acp::{
    process::StdinEndpoint,
    runtime::{ActivationTransport, EgressTransport, StdinTransport},
    transport::{FrameWriteOutcome, OutboundFrame},
};

#[derive(Clone, Copy)]
pub(super) enum ResumeCase {
    Success,
    Failure,
}

impl ResumeCase {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Success => "resume-success-once",
            Self::Failure => "resume-failure-no-second-attempt",
        }
    }
}

#[derive(Default)]
pub(super) struct WireCounts {
    initialize: AtomicUsize,
    new_session: AtomicUsize,
    resume: AtomicUsize,
    prompt: AtomicUsize,
    other: AtomicUsize,
}

impl WireCounts {
    fn observe(&self, frame: &OutboundFrame) {
        let method = serde_json::from_slice::<serde_json::Value>(frame.payload())
            .ok()
            .and_then(|value| value.get("method")?.as_str().map(str::to_owned));
        let counter = match method.as_deref() {
            Some("fixture/initialize") => &self.initialize,
            Some("fixture/newSession") => &self.new_session,
            Some("fixture/resume") => &self.resume,
            Some("fixture/prompt") => &self.prompt,
            _ => &self.other,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn initialize(&self) -> usize {
        self.initialize.load(Ordering::Relaxed)
    }

    pub(super) fn new_session(&self) -> usize {
        self.new_session.load(Ordering::Relaxed)
    }

    pub(super) fn resume(&self) -> usize {
        self.resume.load(Ordering::Relaxed)
    }

    pub(super) fn prompt(&self) -> usize {
        self.prompt.load(Ordering::Relaxed)
    }

    pub(super) fn other(&self) -> usize {
        self.other.load(Ordering::Relaxed)
    }
}

pub(super) struct CompleteTransport {
    counts: Arc<WireCounts>,
}

impl CompleteTransport {
    pub(super) fn new() -> (Self, Arc<WireCounts>) {
        let counts = Arc::new(WireCounts::default());
        (
            Self {
                counts: Arc::clone(&counts),
            },
            counts,
        )
    }
}

impl ActivationTransport for CompleteTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.counts.observe(frame);
        FrameWriteOutcome::Complete
    }
}

pub(super) struct ObservedStdin {
    inner: StdinEndpoint,
    counts: Arc<WireCounts>,
}

impl ObservedStdin {
    pub(super) fn new(inner: StdinEndpoint) -> (Self, Arc<WireCounts>) {
        let counts = Arc::new(WireCounts::default());
        (
            Self {
                inner,
                counts: Arc::clone(&counts),
            },
            counts,
        )
    }

    fn write_observed(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.counts.observe(frame);
        self.inner.write_frame(frame)
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
        self.inner.close();
    }
}
