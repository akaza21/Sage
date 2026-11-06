//! The whole process tree a Git child starts.
//!
//! On Windows `git` on PATH is usually Git for Windows' `cmd\git.exe`, a
//! launcher that starts the real `mingw64\bin\git.exe`, which in turn starts
//! filters, transports and helpers. Terminating only the spawned process
//! leaves all of those running. Every Git child is therefore created
//! suspended, assigned to its own kill-on-close Job Object, and only then
//! resumed, so nothing it starts can escape the job. Killing the tree
//! terminates the job, and an exit is proven only once job accounting shows
//! no active process.
//!
//! Elsewhere the tree is just the spawned process, as before.
//!
//! Git can also run several children at once (parallel checkout workers,
//! parallel fetches). Sage bounds that parallelism on every call with
//! [`parallel_job_overrides`], derived from the same capacity, so a repository
//! that asks for more never reaches the job's process limit.

/// Processes a Git tree may run at once. More fail to start.
const MAX_TREE_PROCESSES: u32 = 32;
/// Slots kept for the root, Git for Windows' launcher, and the filters,
/// credential and askpass helpers that run alongside parallel jobs.
const TREE_HEADROOM: u32 = 8;
/// Processes one parallel job may need: a submodule fetch can be Git, its
/// transport and a helper; a checkout worker is one process.
const PROCESSES_PER_JOB: u32 = 3;
/// The most parallel jobs Git may run, whatever the repository configures.
const MAX_PARALLEL_JOBS: u32 = (MAX_TREE_PROCESSES - TREE_HEADROOM) / PROCESSES_PER_JOB;

const _: () = assert!(TREE_HEADROOM + MAX_PARALLEL_JOBS * PROCESSES_PER_JOB <= MAX_TREE_PROCESSES);

/// Git settings that start parallel children. A value of 0 or less would
/// otherwise mean one per CPU.
const PARALLEL_JOB_SETTINGS: [&str; 3] =
    ["checkout.workers", "fetch.parallel", "submodule.fetchJobs"];

/// `-c` overrides capping each of them at [`MAX_PARALLEL_JOBS`]. Command-line
/// config wins over every config file.
pub(super) fn parallel_job_overrides() -> impl Iterator<Item = String> {
    PARALLEL_JOB_SETTINGS
        .into_iter()
        .map(|setting| format!("{setting}={MAX_PARALLEL_JOBS}"))
}

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

    /// Nothing beyond the root process is tracked here.
    pub(super) fn terminate(&self) -> bool {
        false
    }

    pub(super) fn is_empty(&self) -> bool {
        true
    }
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

    /// One Job Object handle per Git child, closed when the tree is dropped;
    /// closing it kills anything still running.
    pub(in super::super) struct ProcessTree(JobBoundary);

    impl ProcessTree {
        /// Assigns the suspended `child` to a new job, then resumes it.
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

        /// Terminates every process in the tree; true when the job accepted it.
        pub(in super::super) fn terminate(&self) -> bool {
            self.0.terminate().is_ok()
        }

        /// True once job accounting shows no active process. A failed query
        /// proves nothing, so it counts as not empty.
        pub(in super::super) fn is_empty(&self) -> bool {
            self.0
                .active_processes(MAX_TREE_PROCESSES)
                .is_ok_and(|active| active == 0)
        }
    }
}

/// A spawned Git root together with the tree it may start.
pub(super) struct GitChild {
    pub(super) root: tokio::process::Child,
    pub(super) tree: ProcessTree,
}

/// Waits up to `budget` for the tree to empty.
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
