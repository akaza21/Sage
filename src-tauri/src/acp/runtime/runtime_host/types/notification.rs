//! The worker tells the host about a Turn's start and end without waiting
//! on it, since the host may be waiting on that worker; it reads the host's
//! answer later.

use std::{sync::mpsc, time::Instant};

use super::{HostError, HostOutcome, HostReply};

pub(crate) struct TerminalNotification {
    pub(in crate::acp::runtime::runtime_host) response:
        mpsc::Receiver<Result<HostReply, HostError>>,
    pub(in crate::acp::runtime::runtime_host) deadline: Instant,
}

impl TerminalNotification {
    pub(crate) fn wait(self) -> Result<(), HostError> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        match self.response.recv_timeout(remaining) {
            Ok(Ok(HostReply::Outcome(HostOutcome::TurnTerminal))) => Ok(()),
            Ok(Ok(
                HostReply::Outcome(_)
                | HostReply::NativeRpc(_)
                | HostReply::Prepared(_)
                | HostReply::State(_),
            )) => Err(HostError::State),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(HostError::Deadline),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(HostError::Closed),
        }
    }
}

pub(crate) struct StartNotification {
    pub(in crate::acp::runtime::runtime_host) response:
        mpsc::Receiver<Result<HostReply, HostError>>,
    pub(in crate::acp::runtime::runtime_host) deadline: Instant,
}

impl StartNotification {
    /// Whether the host has taken the started Turn as running, read without
    /// waiting. A refusal, or no answer by the deadline, is an error.
    pub(crate) fn settled(&self) -> Result<bool, HostError> {
        match self.response.try_recv() {
            Ok(Ok(HostReply::Outcome(HostOutcome::TurnStarted))) => Ok(true),
            Ok(Ok(_)) => Err(HostError::State),
            Ok(Err(error)) => Err(error),
            Err(mpsc::TryRecvError::Empty) if Instant::now() < self.deadline => Ok(false),
            Err(mpsc::TryRecvError::Empty) => Err(HostError::Deadline),
            Err(mpsc::TryRecvError::Disconnected) => Err(HostError::Closed),
        }
    }
}
