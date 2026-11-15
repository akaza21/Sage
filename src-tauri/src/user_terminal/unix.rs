use std::{
    fs::File,
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use super::{
    TerminalError, TerminalFactory, TerminalOutputSink, TerminalProcess, TerminalSpawnFailure,
};
use crate::project_registry::ProjectRootAuthority;
use shell::{LoginShell, environment, login_shell};

const HANGUP_GRACE: Duration = Duration::from_millis(500);
const SETTLE_DEADLINE: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Writes waiting for the PTY; each is at most `INPUT_BYTES`, so at most
/// 64 KiB is queued before input is refused as busy.
const INPUT_QUEUE: usize = 16;
/// The standard signals. Any Sage ignores would stay ignored after exec, so
/// the shell gets the defaults a terminal emulator gives it.
const STANDARD_SIGNALS: std::ops::Range<libc::c_int> = 1..32;

pub(super) struct UnixPtyFactory;

impl TerminalFactory for UnixPtyFactory {
    fn spawn(
        &self,
        root: &ProjectRootAuthority,
        columns: u16,
        rows: u16,
        output: TerminalOutputSink,
    ) -> Result<Box<dyn TerminalProcess>, TerminalSpawnFailure> {
        root.revalidate().map_err(|_| TerminalError::InvalidScope)?;
        let process = launch(root.directory(), &login_shell()?, columns, rows, output)?;
        Ok(Box::new(process))
    }
}

fn launch(
    root: &File,
    shell: &LoginShell,
    columns: u16,
    rows: u16,
    output: TerminalOutputSink,
) -> Result<UnixPtyProcess, TerminalSpawnFailure> {
    let environment = environment(&shell.declared)?;
    let (master, slave) = pty::open(columns, rows)?;
    let clone = |fd: &OwnedFd| fd.try_clone().map_err(|_| TerminalError::Backend);
    let (reader, writer) = (clone(&master)?, clone(&master)?);
    let name = shell.declared.file_name().ok_or(TerminalError::Backend)?;
    let mut command = Command::new(&shell.executable);
    // A leading "-" in argv[0] asks any POSIX shell for a login shell, as
    // macOS Terminal and VS Code start one.
    command
        .arg0(format!("-{}", name.to_string_lossy()))
        .env_clear()
        .envs(environment)
        .stdin(Stdio::from(clone(&slave)?))
        .stdout(Stdio::from(clone(&slave)?))
        .stderr(Stdio::from(clone(&slave)?));
    let directory = root.as_raw_fd();
    // SAFETY: signal, fchdir, setsid and ioctl are async-signal-safe, and the
    // directory descriptor stays open until exec. Entering the root by
    // descriptor keeps a path swapped after revalidation from redirecting the
    // shell. The shell leads a new session whose controlling terminal is this
    // PTY, so close can signal every member that has not left the session.
    unsafe {
        command.pre_exec(move || {
            for signal in STANDARD_SIGNALS {
                if signal != libc::SIGKILL && signal != libc::SIGSTOP {
                    libc::signal(signal, libc::SIG_DFL);
                }
            }
            if libc::fchdir(directory) < 0
                || libc::setsid() < 0
                || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().map_err(|_| TerminalError::Backend)?;
    // The parent keeps no slave descriptor, so the reader sees the session end.
    drop(command);
    drop(slave);
    let (input, queued) = sync_channel(INPUT_QUEUE);
    let stop = Arc::new(AtomicBool::new(false));
    let mut running = UnixPtyProcess {
        shell_path: shell.declared.to_string_lossy().into_owned(),
        session: child.id() as libc::pid_t,
        child,
        control: Some(master),
        input: Some(input),
        stop: Arc::clone(&stop),
        threads: Vec::new(),
        empty_scans: 0,
        finished: false,
        lost: false,
    };
    let reading = Arc::clone(&stop);
    let started = std::thread::Builder::new()
        .name("sage-terminal-output".into())
        .spawn(move || io::drain(reader, &output, &reading))
        .map(|thread| running.threads.push(thread))
        .and_then(|()| {
            std::thread::Builder::new()
                .name("sage-terminal-input".into())
                .spawn(move || io::feed(writer, &queued, &stop))
        })
        .map(|thread| running.threads.push(thread));
    if started.is_err() {
        return Err(match running.terminate() {
            Ok(()) => TerminalError::Backend.into(),
            // Capacity stays held until the session is proven empty.
            Err(_) => TerminalSpawnFailure::Retained(Box::new(running)),
        });
    }
    Ok(running)
}

struct UnixPtyProcess {
    shell_path: String,
    child: Child,
    /// The shell's pid, which is also its session id.
    session: libc::pid_t,
    control: Option<OwnedFd>,
    input: Option<SyncSender<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    empty_scans: u8,
    /// The session was proven empty and the shell reaped. Its pid may now be
    /// reused, so nothing is signalled again.
    finished: bool,
    /// The shell could no longer be waited on, so it may have been reaped
    /// elsewhere and its session id given to another process. Nothing is
    /// signalled again, and close fails rather than guess.
    lost: bool,
}

impl UnixPtyProcess {
    /// The shell is left unreaped until its session is empty, so neither its pid
    /// nor its session id can be reused while close may still signal them.
    fn exited(&self) -> Result<bool, TerminalError> {
        if self.finished {
            return Ok(true);
        }
        // SAFETY: siginfo_t is plain data, and all-zero is a valid value.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: WNOWAIT leaves the child unreaped; info is a valid out
            // pointer for the one child named.
            let waited = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.session as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if waited == 0 {
                return Ok(exited_pid(&info) != 0);
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err(TerminalError::Backend);
            }
        }
    }

    fn settled(&mut self) -> bool {
        if self.finished {
            return true;
        }
        match self.exited() {
            Ok(true) => {}
            Ok(false) => return false,
            Err(_) => {
                self.lost = true;
                return false;
            }
        }
        let empty = session::members(self.session)
            // The shell's own unreaped exit is not a live member.
            .is_ok_and(|members| members.iter().all(|&pid| pid == self.session));
        // Two empty scans a poll interval apart, so a child that was still
        // being forked during the first is not missed.
        self.empty_scans = if empty {
            self.empty_scans.saturating_add(1)
        } else {
            0
        };
        if self.empty_scans >= 2 && matches!(self.child.try_wait(), Ok(Some(_))) {
            self.finished = true;
        }
        self.finished
    }

    /// A member Sage may not signal, such as a root process started with sudo,
    /// is an error: retrying cannot end it.
    fn sweep(&self, signal: libc::c_int) -> Result<(), TerminalError> {
        if self.lost {
            return Err(TerminalError::Backend);
        }
        session::signal(self.session, signal).map_err(|_| TerminalError::Backend)
    }

    /// Closes every copy of the master, which hangs up the PTY as closing a
    /// terminal window does. The kernel's own SIGHUP and the EIO on further
    /// terminal I/O also reach members that Sage may not signal.
    fn hang_up(&mut self) -> Result<(), TerminalError> {
        self.input.take();
        self.control.take();
        self.stop.store(true, Ordering::Release);
        for thread in self.threads.drain(..) {
            thread.join().map_err(|_| TerminalError::Backend)?;
        }
        Ok(())
    }

    fn wait_until(&mut self, deadline: Instant, done: impl Fn(&mut Self) -> bool) -> bool {
        loop {
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

#[cfg(target_os = "linux")]
fn exited_pid(info: &libc::siginfo_t) -> libc::pid_t {
    // SAFETY: waitid filled the child fields of a zeroed siginfo_t.
    unsafe { info.si_pid() }
}

#[cfg(target_os = "macos")]
fn exited_pid(info: &libc::siginfo_t) -> libc::pid_t {
    info.si_pid
}

impl TerminalProcess for UnixPtyProcess {
    fn shell_path(&self) -> &str {
        &self.shell_path
    }

    /// Queues the bytes for the writer thread and never waits on the PTY, so
    /// a program that stops reading cannot hold Sage's session locks.
    fn write(&mut self, bytes: &[u8]) -> Result<(), TerminalError> {
        if self.exited()? {
            return Err(TerminalError::Ended);
        }
        let input = self.input.as_ref().ok_or(TerminalError::UnknownSession)?;
        match input.try_send(bytes.to_vec()) {
            Ok(()) => Ok(()),
            // Nothing was queued, so the whole write can be sent again.
            Err(TrySendError::Full(_)) => Err(TerminalError::Busy),
            Err(TrySendError::Disconnected(_)) => Err(if self.exited()? {
                TerminalError::Ended
            } else {
                TerminalError::Backend
            }),
        }
    }

    fn resize(&mut self, columns: u16, rows: u16) -> Result<(), TerminalError> {
        let master = self.control.as_ref().ok_or(TerminalError::UnknownSession)?;
        pty::resize(master, columns, rows)
    }

    fn terminate(&mut self) -> Result<(), TerminalError> {
        if !self.settled() {
            // SIGHUP first, so shells save their history, then SIGCONT so a
            // stopped job can run its handler. Job-control shells give each
            // job its own process group, so every process in the session is
            // signalled, not just the shell's group. Both are best effort; the
            // hangup and the SIGKILL sweeps decide.
            let _ = self.sweep(libc::SIGHUP);
            let _ = self.sweep(libc::SIGCONT);
            self.hang_up()?;
            if !self.wait_until(Instant::now() + HANGUP_GRACE, Self::settled) {
                let deadline = Instant::now() + SETTLE_DEADLINE;
                // Repeating the sweep also catches processes forked meanwhile.
                loop {
                    self.sweep(libc::SIGKILL)?;
                    if self.settled() {
                        break;
                    }
                    if Instant::now() >= deadline {
                        // Capacity stays held until the session is proven empty.
                        return Err(TerminalError::Backend);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
            }
        }
        self.hang_up()
    }
}

impl Drop for UnixPtyProcess {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

mod io;
mod pty;
mod session;
mod shell;
#[cfg(test)]
mod tests;
