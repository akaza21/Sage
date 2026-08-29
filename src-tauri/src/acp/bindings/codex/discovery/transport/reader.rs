use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::acp::{
    limits::LIMITS,
    process::StdoutEndpoint,
    transport::{Frame, FrameError},
};

use super::super::runner::CodexDiscoveryRunError;

pub(super) enum ReaderEvent {
    Frame(Frame),
    Eof,
    FramingFailed(FrameError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReaderCompletion {
    Clean,
    Overflow,
}

pub(super) struct ReaderThread {
    pub(super) events: mpsc::Receiver<ReaderEvent>,
    completion: mpsc::Receiver<ReaderCompletion>,
    overflowed: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ReaderThread {
    pub(super) fn spawn(
        mut stdout: StdoutEndpoint,
        slots: usize,
        session_kind: &str,
    ) -> Result<Self, CodexDiscoveryRunError> {
        if !(1..=4).contains(&slots) {
            return Err(CodexDiscoveryRunError::Transport);
        }
        let (send_event, events) = mpsc::sync_channel(slots);
        let (send_completion, completion) = mpsc::sync_channel(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        let reader_overflowed = Arc::clone(&overflowed);
        let session_kind = session_kind.to_owned();
        let handle = thread::Builder::new()
            .name(format!("sage-codex-{session_kind}-stdout"))
            .spawn(move || {
                let completion = loop {
                    let event = match stdout.read_frame() {
                        Ok(Some(frame)) => ReaderEvent::Frame(frame),
                        Ok(None) => ReaderEvent::Eof,
                        Err(error) => {
                            tracing::warn!(
                                ?error,
                                session_kind,
                                "Codex discovery stdout framing failed"
                            );
                            ReaderEvent::FramingFailed(error)
                        }
                    };
                    if let Some(completion) =
                        enqueue_reader_event(&send_event, event, &reader_overflowed)
                    {
                        if completion == ReaderCompletion::Overflow {
                            tracing::warn!(
                                session_kind,
                                "Codex discovery stdout reader exceeded its bounded buffer"
                            );
                        }
                        break completion;
                    }
                };
                let _ = send_completion.send(completion);
            })
            .map_err(|_| CodexDiscoveryRunError::Transport)?;
        Ok(Self {
            events,
            completion,
            overflowed,
            handle: Some(handle),
        })
    }

    pub(super) fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }

    pub(super) fn finish_before(
        &mut self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(LIMITS.deadlines_ms.explicit_close));
        if timeout.is_zero() {
            return Err(CodexDiscoveryRunError::Cleanup);
        }
        let completion = self
            .completion
            .recv_timeout(timeout)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        let Some(handle) = self.handle.take() else {
            return Err(CodexDiscoveryRunError::Cleanup);
        };
        handle.join().map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        match completion {
            ReaderCompletion::Clean => Ok(()),
            ReaderCompletion::Overflow => Err(CodexDiscoveryRunError::Overflow),
        }
    }
}

pub(super) fn enqueue_reader_event(
    sender: &mpsc::SyncSender<ReaderEvent>,
    event: ReaderEvent,
    overflowed: &AtomicBool,
) -> Option<ReaderCompletion> {
    let terminal = !matches!(&event, ReaderEvent::Frame(_));
    match sender.try_send(event) {
        Ok(()) if terminal => Some(ReaderCompletion::Clean),
        Ok(()) => None,
        Err(mpsc::TrySendError::Full(_)) => {
            overflowed.store(true, Ordering::Release);
            Some(ReaderCompletion::Overflow)
        }
        Err(mpsc::TrySendError::Disconnected(_)) => Some(ReaderCompletion::Clean),
    }
}
