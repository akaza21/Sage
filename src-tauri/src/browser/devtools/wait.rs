use crate::acp::browser::BrowserFailure;
use sage_core::limits::BROWSER_LIMITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WaitBudget {
    timeout_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WaitStep {
    Probe,
    TimedOut,
    Cancelled,
}

impl WaitBudget {
    pub(super) fn new(timeout_ms: u32) -> Result<Self, BrowserFailure> {
        if timeout_ms == 0 || timeout_ms > BROWSER_LIMITS.wait_timeout_ms {
            return Err(BrowserFailure::Capacity);
        }
        Ok(Self { timeout_ms })
    }

    pub(super) fn step(self, elapsed_ms: u32, cancelled: bool) -> WaitStep {
        if cancelled {
            WaitStep::Cancelled
        } else if elapsed_ms >= self.timeout_ms {
            WaitStep::TimedOut
        } else {
            WaitStep::Probe
        }
    }
}
