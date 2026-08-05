use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::acp::{
    process::{StdinEndpoint, TreeControl, TreeExit},
    runtime::{
        ActivationTransport, BoundedThread, EgressTransport, StdinTransport, ThreadWaitError,
    },
    transport::{FrameWriteOutcome, OutboundFrame},
};

const READER_DEADLINE: Duration = Duration::from_secs(5);

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

pub(super) struct TempRoot {
    path: PathBuf,
    armed: bool,
}

impl TempRoot {
    pub(super) fn new(prefix: &str) -> Self {
        Self {
            path: std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4())),
            armed: true,
        }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn remove(mut self) {
        std::fs::remove_dir_all(&self.path).unwrap();
        self.armed = false;
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

pub(super) struct ProcessCleanup {
    control: TreeControl,
    reader: Option<BoundedThread>,
    zero: bool,
}

impl ProcessCleanup {
    pub(super) fn new(control: TreeControl, reader: Option<BoundedThread>) -> Self {
        Self {
            control,
            reader,
            zero: false,
        }
    }

    pub(super) fn attach_reader(&mut self, reader: BoundedThread) {
        assert!(self.reader.replace(reader).is_none());
    }

    pub(super) fn finish_after_zero(&mut self) -> Result<(), ThreadWaitError> {
        self.zero = true;
        match &mut self.reader {
            Some(reader) => reader.wait(READER_DEADLINE),
            None => Ok(()),
        }
    }
}

impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        if !self.zero {
            let _ = self.control.request_termination(TreeExit::Cancelled);
            let _ = self.control.wait_zero();
        }
        if let Some(reader) = &mut self.reader {
            let _ = reader.wait(READER_DEADLINE);
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
