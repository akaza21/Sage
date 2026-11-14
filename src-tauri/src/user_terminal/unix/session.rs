//! The processes of one terminal session. The shell leads its own session, and
//! job-control shells move each job to a process group of its own, so close
//! finds members by session id rather than by process group.

use std::io;

/// Bounds one scan of the process table. A scan that reaches it fails rather
/// than reporting a partial list.
const MAX_SCANNED: usize = 65_536;

/// A process that leaves the session (by calling `setsid`, as daemons do) is
/// not signalled. A failed scan returns at once; a member that may not be
/// signalled is an error once every other member has been signalled.
pub(super) fn signal(session: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    let mut result = Ok(());
    for pid in members(session)? {
        // Checking again just before the signal narrows the window in which
        // an exited member's pid could have gone to an unrelated process.
        // SAFETY: getsid only reads a session id and kill only sends a signal.
        let sent = unsafe { libc::getsid(pid) != session || libc::kill(pid, signal) == 0 };
        if !sent {
            let error = io::Error::last_os_error();
            // A member that exited since the scan is not a failure.
            if error.raw_os_error() != Some(libc::ESRCH) {
                result = Err(error);
            }
        }
    }
    result
}

/// An exited process whose threads all ended is left out: it runs nothing, and
/// its reaping belongs to its parent.
#[cfg(target_os = "linux")]
pub(super) fn members(session: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    let mut members = Vec::new();
    if session <= 1 {
        return Ok(members);
    }
    for (scanned, entry) in std::fs::read_dir("/proc")?.enumerate() {
        if scanned >= MAX_SCANNED {
            return Err(io::Error::other("process table scan was cut short"));
        }
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        // A process that exits during the scan has no stat left to read.
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let live = match member_state(&stat, session) {
            // A leader that exited while its other threads still run.
            Some("Z") => std::fs::read_to_string(format!("/proc/{pid}/status"))
                .is_ok_and(|status| thread_count(&status) > 1),
            Some("X") | None => false,
            Some(_) => true,
        };
        if live {
            members.push(pid);
        }
    }
    Ok(members)
}

/// The state of a process in `session`, from `/proc/<pid>/stat`: after the
/// parenthesised command name come the state, parent, process group and
/// session.
#[cfg(target_os = "linux")]
fn member_state(stat: &str, session: libc::pid_t) -> Option<&str> {
    let (_, fields) = stat.rsplit_once(')')?;
    let mut fields = fields.split_whitespace();
    let state = fields.next()?;
    (fields.nth(2)?.parse() == Ok(session)).then_some(state)
}

#[cfg(target_os = "linux")]
fn thread_count(status: &str) -> usize {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|count| count.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
pub(super) fn members(session: libc::pid_t) -> io::Result<Vec<libc::pid_t>> {
    if session <= 1 {
        return Ok(Vec::new());
    }
    // SAFETY: a null buffer asks only for the current process count.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let count = usize::try_from(count).map_err(|_| io::Error::last_os_error())?;
    // Room for processes started since the count.
    let mut pids = vec![0 as libc::pid_t; (count * 2 + 64).min(MAX_SCANNED)];
    let bytes = libc::c_int::try_from(pids.len() * size_of::<libc::pid_t>())
        .map_err(|_| io::Error::other("process list buffer is too large"))?;
    // SAFETY: the buffer holds `bytes` bytes of pid_t slots.
    let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    let filled = usize::try_from(filled).map_err(|_| io::Error::last_os_error())?;
    // A full buffer may have cut the list short.
    if filled >= pids.len() {
        return Err(io::Error::other("process list was cut short"));
    }
    pids.truncate(filled);
    pids.retain(|&pid| {
        // SAFETY: getsid only reads the session id of a process.
        pid > 0 && unsafe { libc::getsid(pid) } == session
    });
    Ok(pids)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{member_state, thread_count};

    #[test]
    fn reads_the_session_state_and_thread_count() {
        assert_eq!(
            member_state("41 (sleep) S 40 41 40 34816 41 0", 40),
            Some("S")
        );
        assert_eq!(member_state("41 (a) b) (c) R 40 41 40 0", 40), Some("R"));
        assert_eq!(member_state("41 (sleep) Z 40 41 40 0", 40), Some("Z"));
        assert_eq!(member_state("41 (sleep) S 40 41 39 0", 40), None);
        assert_eq!(member_state("garbage", 40), None);
        assert_eq!(
            thread_count("Name:\tx\nState:\tZ (zombie)\nThreads:\t3\n"),
            3
        );
        assert_eq!(thread_count("Name:\tx\n"), 0);
    }
}
