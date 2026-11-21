//! Containment is signalled at most once, while the leader is unreaped.
#[cfg(unix)]
use sage_process::process_group::ProcessGroup as TitleTree;
#[cfg(windows)]
use sage_process::process_tree::ProcessTree as TitleTree;
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::process::Child;

pub(super) struct OwnedTree {
    tree: TitleTree,
    stopped: AtomicBool,
}
impl OwnedTree {
    pub(super) fn new(tree: TitleTree) -> Self {
        Self {
            tree,
            stopped: AtomicBool::new(false),
        }
    }
    pub(super) fn terminate(&self) {
        if !self.stopped.swap(true, Ordering::AcqRel) {
            self.tree.terminate();
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }
}
pub(super) struct Process {
    pub(super) child: Child,
    pub(super) tree: Arc<OwnedTree>,
}
impl Drop for Process {
    fn drop(&mut self) {
        self.tree.terminate();
    }
}
pub(super) fn isolate(command: &mut Command) {
    #[cfg(unix)]
    sage_process::process_group::isolate(command);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(
            windows_sys::Win32::System::Threading::CREATE_NO_WINDOW
                | sage_process::process_tree::CREATION_FLAGS,
        );
    }
}
#[cfg(windows)]
pub(super) fn contain(child: &Child) -> std::io::Result<TitleTree> {
    TitleTree::contain(child)
}
#[cfg(unix)]
pub(super) fn contain(child: &Child) -> std::io::Result<TitleTree> {
    TitleTree::led_by(
        child
            .id()
            .ok_or_else(|| std::io::Error::other("Pi already exited"))?,
    )
}
#[cfg(windows)]
pub(super) fn exit_code(child: &Child) -> Option<i32> {
    use windows_sys::Win32::System::Threading::GetExitCodeProcess;
    let mut code = 259;
    // SAFETY: the unreaped child owns this live process handle.
    let read = unsafe { GetExitCodeProcess(child.raw_handle()? as _, &mut code) };
    (read != 0 && code != 259).then_some(code as i32)
}
#[cfg(unix)]
pub(super) fn exit_code(child: &Child) -> Option<i32> {
    // SAFETY: siginfo_t has a valid all-zero representation.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: valid output storage; WNOWAIT preserves ownership of our child.
    let read = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id()?,
            &raw mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    // SAFETY: waitid initialized the child status fields on a successful read.
    (read == 0 && unsafe { info.si_pid() } != 0).then(|| unsafe { info.si_status() })
}
