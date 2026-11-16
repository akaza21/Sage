//! On Unix a child started from an [`isolate`]d command leads a new process
//! group, and everything it starts joins that group unless it leaves on
//! purpose (`setsid`, `setpgid`). Killing the group ends a launcher together
//! with the children that still hold its pipes.
//!
//! Invariant: a group is signalled only while its leader is unreaped. Once
//! the leader is reaped and its group empties, the id may be reused.

use std::{io, os::unix::process::CommandExt, process::Command};

/// Starts the command's child as the leader of a new process group.
pub fn isolate(command: &mut Command) {
    command.process_group(0);
}

/// The process group a child of an [`isolate`]d command leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessGroup(libc::pid_t);

impl ProcessGroup {
    /// `leader` is the child's id; the group exists only if the child was
    /// started from an [`isolate`]d command.
    pub fn led_by(leader: u32) -> io::Result<Self> {
        libc::pid_t::try_from(leader)
            .ok()
            .filter(|&leader| leader > 0)
            .map(Self)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))
    }

    /// Kills every process still in the group; true when the signal was sent.
    pub fn terminate(&self) -> bool {
        // SAFETY: killpg only sends a signal; the id names a group this
        // process created, and callers hold its leader unreaped.
        unsafe { libc::killpg(self.0, libc::SIGKILL) == 0 }
    }

    /// True once no process, not even an unreaped zombie, is left in the
    /// group. Signal 0 delivers nothing, so it may also ask after the leader
    /// is reaped: a reused id can only make the group look occupied.
    pub fn is_empty(&self) -> bool {
        // SAFETY: signal 0 only checks whether the group has members.
        let probed = unsafe { libc::killpg(self.0, 0) };
        probed != 0 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader},
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };

    use super::*;

    fn alive(pid: libc::pid_t) -> bool {
        // SAFETY: signal 0 only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn killing_the_group_ends_the_children_its_leader_started() {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 30 & echo $!; wait"])
            .stdout(Stdio::piped());
        isolate(&mut command);
        let mut leader = command.spawn().unwrap();
        let group = ProcessGroup::led_by(leader.id()).unwrap();
        let mut line = String::new();
        BufReader::new(leader.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant: libc::pid_t = line.trim().parse().unwrap();
        assert!(alive(descendant));

        assert!(group.terminate());
        leader.wait().unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while alive(descendant) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(!alive(descendant), "the group's descendant survived");
    }

    #[test]
    fn a_group_is_empty_only_once_its_last_member_is_reaped() {
        let mut command = Command::new("/bin/sleep");
        command.arg("30");
        isolate(&mut command);
        let mut leader = command.spawn().unwrap();
        let group = ProcessGroup::led_by(leader.id()).unwrap();
        let mut member = Command::new("/bin/sleep")
            .arg("30")
            .process_group(libc::pid_t::try_from(leader.id()).unwrap())
            .spawn()
            .unwrap();
        assert!(!group.is_empty());

        assert!(group.terminate());
        leader.wait().unwrap();
        assert!(!group.is_empty(), "an unreaped member still occupies it");

        member.wait().unwrap();
        assert!(group.is_empty());
    }

    #[test]
    fn a_child_id_that_is_not_a_process_id_names_no_group() {
        assert!(ProcessGroup::led_by(0).is_err());
        assert!(ProcessGroup::led_by(u32::MAX).is_err());
    }
}
