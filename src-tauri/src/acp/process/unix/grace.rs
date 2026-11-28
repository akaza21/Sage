use std::time::{Duration, Instant};

use sage_process::error::LaunchError;

use super::containment::TreeControl;
use crate::acp::process::{
    graceful_exit::{self, ExitingProcess},
    supervisor::TreeExit,
};

impl TreeControl {
    /// Records `reason` and leaves the root, whose stdin the caller has
    /// already closed, `grace` to exit on its own. The next wait for the tree
    /// kills the root if it is still running then.
    pub(crate) fn request_graceful_termination(
        &self,
        reason: TreeExit,
        grace: Duration,
    ) -> Result<(), LaunchError> {
        let mut child = self
            .0
            .child
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        let status = child.try_wait().ok().flatten();
        self.0
            .termination
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .request_graceful(
                reason,
                || {
                    status.map(|status| TreeExit::Exited {
                        code: status.code(),
                    })
                },
                Instant::now() + grace,
            );
        Ok(())
    }

    /// When this wait lasts past a pending grace, lets the grace run out and
    /// kills the root then if it has not exited by itself.
    pub(super) fn finish_grace(&self, deadline: Instant) -> Result<(), LaunchError> {
        let Some(kill_at) = self
            .0
            .termination
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .claim_grace(deadline)
        else {
            return Ok(());
        };
        graceful_exit::exit_or_kill(&mut Root(self), kill_at);
        Ok(())
    }

    /// The native bound on a wait for the tree, past any pending grace.
    pub(super) fn native_wait_deadline(&self) -> Result<Instant, LaunchError> {
        Ok(self
            .0
            .termination
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .wait_deadline(self.0.terminate_deadline))
    }
}

struct Root<'a>(&'a TreeControl);

impl ExitingProcess for Root<'_> {
    fn exited(&mut self) -> bool {
        self.0
            .0
            .child
            .lock()
            .map_or(true, |mut child| !matches!(child.try_wait(), Ok(None)))
    }

    fn kill(&mut self) {
        if let Ok(mut child) = self.0.0.child.lock() {
            let _ = child.kill();
        }
    }
}
