use std::time::{Duration, Instant};

use sage_process::error::LaunchError;

use super::{TreeControl, locked, try_observe_exit};
use crate::acp::process::{
    graceful_exit::{self, ExitingProcess},
    supervisor::TreeExit,
};

impl TreeControl {
    /// Records `reason` and leaves the root, whose stdin the caller has
    /// already closed, `grace` to exit on its own. The next wait for the tree
    /// kills the job if the root is still running then.
    pub(crate) fn request_graceful_termination(
        &self,
        reason: TreeExit,
        grace: Duration,
    ) -> Result<(), LaunchError> {
        locked(&self.0.termination)?.request_graceful(
            reason,
            || self.native_exit_if_exited(),
            Instant::now() + grace,
        );
        Ok(())
    }

    /// Lets a pending grace run until `deadline` at most, then kills the job
    /// if the root has not exited by itself.
    pub(super) fn finish_grace(&self, deadline: Instant) -> Result<(), LaunchError> {
        let Some(kill_at) = locked(&self.0.termination)?.take_kill_at() else {
            return Ok(());
        };
        let mut root = Root {
            control: self,
            killed: Ok(()),
        };
        graceful_exit::exit_or_kill(&mut root, kill_at.min(deadline));
        root.killed
    }

    #[cfg(test)]
    pub(crate) fn terminate(&self, reason: TreeExit) -> Result<TreeExit, LaunchError> {
        let deadline = Instant::now() + self.0.terminate_deadline;
        self.request_termination(reason)?;
        self.wait_until(deadline)
    }

    /// The native bound on a wait for the tree, past any pending grace.
    pub(super) fn native_wait_deadline(&self) -> Result<Instant, LaunchError> {
        Ok(locked(&self.0.termination)?.wait_deadline(self.0.terminate_deadline))
    }
}

struct Root<'a> {
    control: &'a TreeControl,
    killed: Result<(), LaunchError>,
}

impl ExitingProcess for Root<'_> {
    fn exited(&mut self) -> bool {
        try_observe_exit(self.control.0.process.raw()).unwrap_or(true)
    }

    fn kill(&mut self) {
        self.killed = self.control.0.job.terminate();
    }
}
