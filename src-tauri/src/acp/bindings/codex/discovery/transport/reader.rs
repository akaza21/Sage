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
    events: Option<mpsc::Receiver<ReaderEvent>>,
    completion: mpsc::Receiver<ReaderCompletion>,
    overflowed: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ReaderThread {
    pub(super) fn spawn(
        mut stdout: StdoutEndpoint,
        slots: usize,
        session_kind: &str,
        deadline: Instant,
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
                        enqueue_reader_event(&send_event, event, &reader_overflowed, deadline)
                    {
                        if completion == ReaderCompletion::Overflow {
                            tracing::warn!(
                                session_kind,
                                "Codex discovery stdout reader buffer stayed full until its deadline"
                            );
                        }
                        break completion;
                    }
                };
                let _ = send_completion.send(completion);
            })
            .map_err(|_| CodexDiscoveryRunError::Transport)?;
        Ok(Self {
            events: Some(events),
            completion,
            overflowed,
            handle: Some(handle),
        })
    }

    pub(super) fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }

    pub(super) fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> Result<ReaderEvent, mpsc::RecvTimeoutError> {
        let event = self
            .events
            .as_ref()
            .ok_or(mpsc::RecvTimeoutError::Disconnected)?
            .recv_timeout(timeout);
        self.wake_reader();
        event
    }

    fn wake_reader(&self) {
        if let Some(handle) = &self.handle {
            handle.thread().unpark();
        }
    }

    fn close_events(&mut self) {
        // Drop before waking: the producer must observe disconnection even if
        // it has not parked yet. This also releases a full queue during cleanup.
        self.events.take();
        self.wake_reader();
    }

    pub(super) fn finish_before(
        &mut self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.close_events();
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

impl Drop for ReaderThread {
    fn drop(&mut self) {
        self.close_events();
    }
}

pub(super) fn enqueue_reader_event(
    sender: &mpsc::SyncSender<ReaderEvent>,
    mut event: ReaderEvent,
    overflowed: &AtomicBool,
    deadline: Instant,
) -> Option<ReaderCompletion> {
    let terminal = !matches!(&event, ReaderEvent::Frame(_));
    loop {
        match sender.try_send(event) {
            Ok(()) if terminal => return Some(ReaderCompletion::Clean),
            Ok(()) => return None,
            Err(mpsc::TrySendError::Full(pending)) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    overflowed.store(true, Ordering::Release);
                    return Some(ReaderCompletion::Overflow);
                }
                event = pending;
                // At most the bounded channel plus this one frame is retained.
                // A receive or receiver drop unparks us; the token prevents a
                // lost wakeup between try_send and park_timeout.
                thread::park_timeout(remaining);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return Some(ReaderCompletion::Clean),
        }
    }
}

#[cfg(test)]
mod tests;
