use std::ffi::OsStr;

use super::directory_skip::skip_workspace_directory_name;

pub const MAX_WORKSPACE_FILES: u64 = 200_000;
pub const MAX_WORKSPACE_QUEUED_DIRECTORIES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkBufferAdmit {
    Skip,
    Keep,
    Cap,
}

pub fn walk_over_cap(seen: u64, cap: u64) -> bool {
    seen > cap
}

pub fn walk_buffer_remaining(seen: u64, cap: u64) -> usize {
    usize::try_from(cap.saturating_sub(seen)).unwrap_or(usize::MAX)
}

pub fn admit_walk_dirent(name: &OsStr, buffered: usize, remaining: usize) -> WalkBufferAdmit {
    if skip_workspace_directory_name(name) {
        return WalkBufferAdmit::Skip;
    }
    if remaining == 0 || buffered >= remaining {
        return WalkBufferAdmit::Cap;
    }
    WalkBufferAdmit::Keep
}

pub fn admit_queued_directory(queued: usize, cap: usize) -> bool {
    queued < cap
}
