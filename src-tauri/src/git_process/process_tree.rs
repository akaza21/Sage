//! On Windows `git` on PATH is usually Git for Windows' `cmd\git.exe`, a
//! launcher that starts the real `mingw64\bin\git.exe`, which in turn starts
//! filters, transports and helpers. Terminating only the spawned process
//! leaves all of those running. Every Git child is therefore created
//! suspended, assigned to its own kill-on-close Job Object, and only then
//! resumed, so nothing it starts can escape the job. Killing the tree
//! terminates the job, and an exit is proven only once job accounting shows
//! no active process.
//!
//! Git can also run several children at once (parallel checkout workers,
//! parallel fetches). [`MAX_PARALLEL_JOBS`] is derived from the same capacity,
//! and `parallel_limits` caps any repository that asks for more, so the job's
//! process limit is never what stops Git.

/// Processes a Git tree may run at once. More fail to start.
pub(super) const MAX_TREE_PROCESSES: u32 = 32;
/// Slots kept for the root, Git for Windows' launcher, and the filters,
/// credential and askpass helpers that run alongside parallel jobs.
pub(super) const TREE_HEADROOM: u32 = 8;
/// Processes one parallel job may need: a submodule fetch can be Git, its
/// transport and a helper; a checkout worker is one process.
pub(super) const PROCESSES_PER_JOB: u32 = 3;
pub(super) const MAX_PARALLEL_JOBS: u32 = (MAX_TREE_PROCESSES - TREE_HEADROOM) / PROCESSES_PER_JOB;

const _: () = assert!(TREE_HEADROOM + MAX_PARALLEL_JOBS * PROCESSES_PER_JOB <= MAX_TREE_PROCESSES);

#[cfg(not(windows))]
use std::io;

#[cfg(not(windows))]
use tokio::process::Child;

#[cfg(windows)]
pub(super) use windows::{CREATION_FLAGS, ProcessTree};

#[cfg(not(windows))]
pub(super) struct ProcessTree;

#[cfg(not(windows))]
impl ProcessTree {
    pub(super) fn contain(_child: &Child) -> io::Result<Self> {
        Ok(Self)
    }

    pub(super) fn terminate(&self) -> bool {
        false
    }

    pub(super) fn is_empty(&self) -> bool {
        true
    }

    pub(super) fn close(self) {}
}

#[cfg(windows)]
mod windows {
    use std::io;

    use tokio::process::Child;
    use windows_sys::Win32::{Foundation::HANDLE, System::Threading::CREATE_SUSPENDED};

    use crate::acp::process::TerminalJobBoundary as JobBoundary;

    use super::MAX_TREE_PROCESSES;

    /// Added to Git's creation flags so it cannot run before containment.
    pub(in super::super) const CREATION_FLAGS: u32 = CREATE_SUSPENDED;

    #[link(name = "ntdll")]
    unsafe extern "system" {
        /// Resumes every thread of a process; a process created suspended has
        /// exactly its primary thread, which std does not expose.
        fn NtResumeProcess(process: HANDLE) -> i32;
    }

    pub(in super::super) struct ProcessTree(JobBoundary);

    impl ProcessTree {
        pub(in super::super) fn contain(child: &Child) -> io::Result<Self> {
            let process = child
                .raw_handle()
                .ok_or_else(|| io::Error::other("the suspended Git process has no handle"))?
                as HANDLE;
            let job = JobBoundary::create(MAX_TREE_PROCESSES)
                .map_err(|_| io::Error::other("the Git Job Object could not be created"))?;
            job.assign(process)
                .map_err(|_| io::Error::other("Git could not be assigned to its Job Object"))?;
            // SAFETY: `process` is the live handle tokio owns for `child`.
            if unsafe { NtResumeProcess(process) } < 0 {
                let _ = job.terminate();
                return Err(io::Error::other(
                    "the contained Git process could not resume",
                ));
            }
            Ok(Self(job))
        }

        pub(in super::super) fn terminate(&self) -> bool {
            self.0.terminate().is_ok()
        }

        pub(in super::super) fn close(self) {}

        /// A failed query proves nothing, so it counts as not empty.
        pub(in super::super) fn is_empty(&self) -> bool {
            self.0
                .active_processes(MAX_TREE_PROCESSES)
                .is_ok_and(|active| active == 0)
        }
    }
}

pub(super) struct GitChild {
    pub(super) root: tokio::process::Child,
    pub(super) tree: ProcessTree,
}

pub(super) async fn wait_until_empty(tree: &ProcessTree, budget: std::time::Duration) -> bool {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        if tree.is_empty() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

#[cfg(all(test, windows))]
mod tests;
