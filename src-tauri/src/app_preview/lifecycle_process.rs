#[cfg(not(windows))]
use std::process::Child;
use std::{
    io::Read,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::acp::limits::PREVIEW_LIMITS;

use super::{
    discovery::PreviewTargetRef,
    lifecycle::starting_exited,
    url::{normalized_origin, parse_allowed_preview_url},
};

const STARTUP_WAIT: Duration = Duration::from_millis(PREVIEW_LIMITS.startup_deadline_ms);
const TERMINATION_WAIT: Duration =
    Duration::from_millis(PREVIEW_LIMITS.process_termination_deadline_ms);
const MAX_STARTUP_OUTPUT_BYTES: usize = PREVIEW_LIMITS.startup_output_bytes as usize;

pub(super) struct ManagedChild {
    #[cfg(not(windows))]
    pub(super) child: Child,
    #[cfg(windows)]
    pub(super) child: super::lifecycle_windows::PreviewProcess,
    drainers: Vec<JoinHandle<()>>,
    #[cfg(unix)]
    process_group: i32,
}

impl ManagedChild {
    #[cfg(not(windows))]
    pub(super) fn reserve(child: Child) -> Result<Self, Child> {
        let Ok(process_group) = i32::try_from(child.id()) else {
            return Err(child);
        };
        Ok(Self {
            child,
            drainers: Vec::with_capacity(PREVIEW_LIMITS.output_drainers_per_child as usize),
            process_group,
        })
    }

    #[cfg(windows)]
    pub(super) fn reserved(child: super::lifecycle_windows::PreviewProcess) -> Self {
        Self {
            child,
            drainers: Vec::with_capacity(PREVIEW_LIMITS.output_drainers_per_child as usize),
        }
    }

    pub(super) fn attach_drainers(
        &mut self,
        drainers: Vec<JoinHandle<()>>,
        deadline: Instant,
    ) -> Result<(), Vec<JoinHandle<()>>> {
        if drainers.len() != PREVIEW_LIMITS.output_drainers_per_child as usize
            || !self.drainers.is_empty()
            || Instant::now() >= deadline
        {
            return Err(drainers);
        }
        self.drainers = drainers;
        Ok(())
    }

    pub(super) fn terminate_and_reap(&mut self) -> bool {
        let deadline = Instant::now() + TERMINATION_WAIT;
        // The root may have exited while a descendant still owns an output
        // pipe. Terminate the exact owned Job/process group before observing
        // the root so tree cleanup never depends on root liveness.
        #[cfg(windows)]
        if !self.child.terminate_and_wait_zero() {
            return false;
        }
        #[cfg(unix)]
        terminate_process_group(self.process_group);
        #[cfg(all(not(unix), not(windows)))]
        let _ = self.child.kill();

        #[cfg(windows)]
        let reaped = self.child.exited();
        #[cfg(not(windows))]
        let reaped = loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
                }
                _ => break false,
            }
        };
        reaped && self.join_drainers_before(deadline)
    }

    pub(super) fn exited(&mut self) -> bool {
        #[cfg(windows)]
        return self.child.exited();
        #[cfg(not(windows))]
        self.child.try_wait().ok().flatten().is_some()
    }

    fn join_drainers_before(&mut self, deadline: Instant) -> bool {
        while self.drainers.iter().any(|drainer| !drainer.is_finished()) {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
        }
        for drainer in self.drainers.drain(..) {
            if drainer.join().is_err() {
                return false;
            }
        }
        true
    }
}

pub(super) fn join_unattached_drainers_before(
    drainers: Vec<JoinHandle<()>>,
    deadline: Instant,
) -> Result<(), Vec<JoinHandle<()>>> {
    while drainers.iter().any(|drainer| !drainer.is_finished()) {
        if Instant::now() >= deadline {
            return Err(drainers);
        }
        thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
    }
    for drainer in drainers {
        if drainer.join().is_err() {
            return Err(Vec::new());
        }
    }
    Ok(())
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        // A prior deadline is not terminal: retained ownership gives Drop one
        // final bounded convergence attempt without pretending it succeeded.
        let _ = self.terminate_and_reap();
    }
}

#[cfg(not(windows))]
pub(super) fn kill_and_reap_unreserved(child: &mut Child) -> bool {
    let _ = child.kill();
    let deadline = Instant::now() + TERMINATION_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(PREVIEW_LIMITS.health_retry_ms));
            }
            _ => return false,
        }
    }
}

#[cfg(unix)]
fn terminate_process_group(process_group: i32) {
    const SIGKILL: i32 = 9;
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: the negative pid targets only the process group created for this
    // owned Preview root, whose child id was range-checked when reserved.
    let _ = unsafe { kill(-process_group, SIGKILL) };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StartupObservation {
    Origin(String),
    OriginNotLoopback,
    ExitedEarly,
    Timeout,
}

pub(super) fn wait_for_origin(
    target: &PreviewTargetRef,
    stdout: impl Read + Send + 'static,
    stderr: impl Read + Send + 'static,
) -> (StartupObservation, Vec<JoinHandle<()>>) {
    let (sender, receiver) = mpsc::sync_channel(PREVIEW_LIMITS.startup_output_channel as usize);
    let drainers = vec![
        drain_preview_output(stdout, sender.clone()),
        drain_preview_output(stderr, sender),
    ];
    let observation =
        wait_for_startup_observation(&receiver, Instant::now() + STARTUP_WAIT, || {
            starting_exited(target)
        });
    (observation, drainers)
}

fn wait_for_startup_observation(
    receiver: &mpsc::Receiver<StartupObservation>,
    deadline: Instant,
    mut exited: impl FnMut() -> bool,
) -> StartupObservation {
    loop {
        let now = Instant::now();
        if now >= deadline {
            break if exited() {
                StartupObservation::ExitedEarly
            } else {
                StartupObservation::Timeout
            };
        }
        let wait = Duration::from_millis(PREVIEW_LIMITS.health_retry_ms)
            .min(deadline.saturating_duration_since(now));
        match receiver.recv_timeout(wait) {
            Ok(observation) => break observation,
            Err(mpsc::RecvTimeoutError::Disconnected) if exited() => {
                break StartupObservation::ExitedEarly;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(wait),
            Err(mpsc::RecvTimeoutError::Timeout) if exited() => {
                break StartupObservation::ExitedEarly;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn drain_preview_output(
    mut reader: impl Read + Send + 'static,
    sender: mpsc::SyncSender<StartupObservation>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0_u8; 1024];
        let mut prefix = String::with_capacity(MAX_STARTUP_OUTPUT_BYTES);
        let mut notified = false;
        while let Ok(read) = reader.read(&mut bytes) {
            if read == 0 {
                break;
            }
            let decoded = String::from_utf8_lossy(&bytes[..read]);
            if decoded.len() >= MAX_STARTUP_OUTPUT_BYTES {
                prefix.clear();
                prefix.push_str(trailing_utf8(&decoded, MAX_STARTUP_OUTPUT_BYTES));
            } else {
                let overflow = prefix
                    .len()
                    .saturating_add(decoded.len())
                    .saturating_sub(MAX_STARTUP_OUTPUT_BYTES);
                if overflow > 0 {
                    let mut split = overflow;
                    while !prefix.is_char_boundary(split) {
                        split += 1;
                    }
                    prefix.drain(..split);
                }
                prefix.push_str(&decoded);
            }
            if !notified && let Some(observation) = parse_origin_observation(&prefix) {
                let _ = sender.try_send(observation);
                notified = true;
            }
        }
    })
}

fn trailing_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut start = value.len() - max_bytes;
    while !value.is_char_boundary(start) {
        start += 1;
    }
    &value[start..]
}

fn parse_origin_observation(output: &str) -> Option<StartupObservation> {
    for token in output.split(|character: char| {
        character.is_whitespace() || matches!(character, '\'' | '"' | '(' | ')' | ',')
    }) {
        let Some(start) = token.find("http") else {
            continue;
        };
        let candidate = token.get(start..)?.trim_end_matches(['/', '.', ':']);
        return Some(match parse_allowed_preview_url(candidate) {
            Ok(url) => StartupObservation::Origin(normalized_origin(&url)),
            Err(_) => StartupObservation::OriginNotLoopback,
        });
    }
    None
}

#[cfg(test)]
pub(super) fn test_startup_output_bytes_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.startup_output_bytes
}

#[cfg(test)]
pub(super) fn test_deadline_millis_admitted(value: u64, limit: u64) -> bool {
    value <= limit
}

#[cfg(test)]
pub(super) fn test_channel_slots_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.startup_output_channel
}

#[cfg(test)]
pub(super) fn test_drainer_count_admitted(value: u64) -> bool {
    value <= PREVIEW_LIMITS.output_drainers_per_child
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        sync::mpsc,
        time::{Duration, Instant},
    };

    use super::{
        StartupObservation, join_unattached_drainers_before, wait_for_startup_observation,
    };

    #[test]
    fn disconnected_output_waits_for_late_root_exit() {
        let (sender, receiver) = mpsc::sync_channel(1);
        drop(sender);
        let probes = Cell::new(0_u8);

        let observation = wait_for_startup_observation(
            &receiver,
            Instant::now() + Duration::from_millis(200),
            || {
                probes.set(probes.get() + 1);
                probes.get() >= 2
            },
        );

        assert_eq!(observation, StartupObservation::ExitedEarly);
        assert!(probes.get() >= 2);
    }

    #[test]
    fn unattached_drainer_deadline_retains_the_join_handle_for_later_cleanup() {
        let (release, wait) = mpsc::sync_channel(1);
        let handle = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        let retained = join_unattached_drainers_before(vec![handle], Instant::now()).unwrap_err();
        assert_eq!(retained.len(), 1);
        release.send(()).unwrap();
        assert!(
            join_unattached_drainers_before(retained, Instant::now() + Duration::from_secs(1),)
                .is_ok()
        );
    }

    #[test]
    fn persistent_unattached_drainer_failure_remains_truthful_and_owned() {
        let (release, wait) = mpsc::sync_channel(1);
        let handle = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        let retained = join_unattached_drainers_before(vec![handle], Instant::now()).unwrap_err();
        assert_eq!(retained.len(), 1, "deadline cannot detach the drainer");
        release.send(()).unwrap();
        assert!(
            join_unattached_drainers_before(retained, Instant::now() + Duration::from_secs(1))
                .is_ok()
        );
    }
}
