use std::{
    collections::BTreeMap,
    io::Read,
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewTargetRef},
    error::PreviewError,
    health, lifecycle_registry,
    url::{normalized_origin, parse_allowed_preview_url},
};

const STARTUP_WAIT: Duration = Duration::from_secs(5);
const MAX_STARTUP_OUTPUT_BYTES: usize = 4096;

struct ActivePreview {
    target: PreviewTargetRef,
    origin: String,
    child: ManagedChild,
}

struct StartingPreview {
    target: PreviewTargetRef,
    child: ManagedChild,
}

#[derive(Default)]
pub(super) struct Registry {
    discovered: BTreeMap<PreviewTargetRef, std::path::PathBuf>,
    starting: Option<StartingPreview>,
    active: Option<ActivePreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStartResponse {
    #[serde(rename = "ref")]
    pub ref_: PreviewTargetRef,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStopResponse {
    #[serde(rename = "ref")]
    pub ref_: PreviewTargetRef,
}

pub(crate) struct PreviewStartReservation {
    target: PreviewTargetRef,
    stdout: ChildStdout,
    stderr: ChildStderr,
}

pub(crate) fn replace_discovered(
    _request: &PreviewDiscoveryRequest,
    targets: Vec<(PreviewTargetRef, std::path::PathBuf)>,
) {
    lifecycle_registry::with_registry(|registry| {
        registry.discovered.clear();
        registry.discovered.extend(targets);
    });
}

pub(crate) fn reserve_start(
    target: PreviewTargetRef,
) -> Result<PreviewStartReservation, PreviewError> {
    let (stdout, stderr) = {
        lifecycle_registry::with_registry(|registry| {
            if registry.active.is_some() || registry.starting.is_some() {
                return Err(PreviewError::conflict(
                    "activePreview",
                    "A Preview target is already active.",
                ));
            }
            let package_root = registry.discovered.get(&target).cloned().ok_or_else(|| {
                PreviewError::conflict(
                    "previewTarget",
                    "Preview target must be discovered before it can start.",
                )
            })?;
            let mut child = Command::new(npm_executable())
                .args(["run", target.script_name.as_str()])
                .current_dir(package_root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| PreviewError::Unavailable {
                    message: "Preview could not start its curated package script.".into(),
                })?;
            let stdout = child.stdout.take().expect("preview stdout was piped");
            let stderr = child.stderr.take().expect("preview stderr was piped");
            let child = match ManagedChild::reserve(child) {
                Ok(child) => child,
                Err(mut child) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PreviewError::Unavailable {
                        message: "Preview could not reserve its process tree.".into(),
                    });
                }
            };
            registry.starting = Some(StartingPreview {
                target: target.clone(),
                child,
            });
            Ok((stdout, stderr))
        })?
    };
    Ok(PreviewStartReservation {
        target,
        stdout,
        stderr,
    })
}

pub(crate) fn finish_start(
    PreviewStartReservation {
        target,
        stdout,
        stderr,
    }: PreviewStartReservation,
) -> Result<PreviewStartResponse, PreviewError> {
    let origin = wait_for_loopback_server(stdout, stderr);
    let Some(origin) = origin else {
        stop_starting(&target);
        return Err(PreviewError::Unavailable {
            message: "Preview script did not expose a healthy loopback server in time.".into(),
        });
    };
    let (promoted, child_to_stop) = {
        lifecycle_registry::with_registry(|registry| {
            let still_current = registry
                .starting
                .as_ref()
                .is_some_and(|starting| matches_target(&starting.target, &target))
                && registry.discovered.contains_key(&target)
                && registry.active.is_none();
            if still_current {
                let StartingPreview { child, .. } =
                    registry.starting.take().expect("starting preview");
                registry.active = Some(ActivePreview {
                    target: target.clone(),
                    origin: origin.clone(),
                    child,
                });
                (true, None)
            } else if registry
                .starting
                .as_ref()
                .is_some_and(|starting| matches_target(&starting.target, &target))
            {
                let child = registry
                    .starting
                    .take()
                    .expect("matching starting preview")
                    .child;
                (false, Some(child))
            } else {
                (false, None)
            }
        })
    };
    if let Some(mut child) = child_to_stop {
        stop_child(&mut child);
    }
    if !promoted {
        return Err(PreviewError::conflict(
            "activePreview",
            "A Preview target is already active.",
        ));
    }
    Ok(PreviewStartResponse {
        ref_: target,
        origin,
    })
}

pub(crate) fn health(target: &PreviewTargetRef) -> Option<String> {
    lifecycle_registry::with_registry(|registry| {
        let active = registry.active.as_mut()?;
        if !matches_target(&active.target, target) {
            return None;
        }
        if active.child.child.try_wait().ok().flatten().is_some() {
            registry.active = None;
            return None;
        }
        if health::get(Some(&active.origin)) {
            Some(active.origin.clone())
        } else {
            None
        }
    })
}

pub(crate) fn stop(target: &PreviewTargetRef) -> Result<PreviewStopResponse, PreviewError> {
    let mut child = lifecycle_registry::with_registry(|registry| {
        if registry
            .active
            .as_ref()
            .is_some_and(|active| matches_target(&active.target, target))
        {
            Ok(registry
                .active
                .take()
                .expect("matching active preview")
                .child)
        } else if registry
            .starting
            .as_ref()
            .is_some_and(|starting| matches_target(&starting.target, target))
        {
            Ok(registry
                .starting
                .take()
                .expect("matching starting preview")
                .child)
        } else if registry.active.is_some() || registry.starting.is_some() {
            Err(PreviewError::conflict(
                "previewTarget",
                "Preview target is not active.",
            ))
        } else {
            Err(PreviewError::conflict(
                "activePreview",
                "No Preview target is active.",
            ))
        }
    })?;
    stop_child(&mut child);
    Ok(PreviewStopResponse {
        ref_: target.clone(),
    })
}

pub(crate) fn stop_before_epoch(new_epoch: u64) {
    let active = {
        lifecycle_registry::with_registry(|registry| {
            registry
                .discovered
                .retain(|target, _| target.expected_transition_epoch >= new_epoch);
            let should_stop_starting = registry
                .starting
                .as_ref()
                .is_some_and(|starting| starting.target.expected_transition_epoch < new_epoch);
            if should_stop_starting {
                let starting = registry.starting.as_mut().expect("starting preview");
                stop_child(&mut starting.child);
                registry.starting = None;
            }
            match registry.active.take() {
                Some(active) if active.target.expected_transition_epoch < new_epoch => Some(active),
                Some(active) => {
                    registry.active = Some(active);
                    None
                }
                None => None,
            }
        })
    };
    if let Some(mut active) = active {
        stop_child(&mut active.child);
    }
}

fn stop_starting(target: &PreviewTargetRef) {
    lifecycle_registry::with_registry(|registry| {
        if registry
            .starting
            .as_ref()
            .is_some_and(|starting| matches_target(&starting.target, target))
        {
            let starting = registry.starting.as_mut().expect("starting preview");
            stop_child(&mut starting.child);
            registry.starting = None;
        }
    });
}

fn matches_target(left: &PreviewTargetRef, right: &PreviewTargetRef) -> bool {
    left.project_id == right.project_id
        && left.project_generation == right.project_generation
        && left.expected_transition_epoch == right.expected_transition_epoch
        && left.root_id == right.root_id
        && left.script_name == right.script_name
}

struct ManagedChild {
    child: Child,
    #[cfg(windows)]
    job: super::lifecycle_windows::PreviewJob,
}

impl ManagedChild {
    fn reserve(child: Child) -> Result<Self, Child> {
        #[cfg(windows)]
        {
            match super::lifecycle_windows::PreviewJob::new(&child) {
                Ok(job) => Ok(Self { child, job }),
                Err(()) => Err(child),
            }
        }
        #[cfg(not(windows))]
        {
            Ok(Self { child })
        }
    }
}

fn stop_child(child: &mut ManagedChild) {
    #[cfg(windows)]
    child.job.terminate();
    #[cfg(not(windows))]
    let _ = child.child.kill();
    let _ = child.child.wait();
}

fn npm_executable() -> &'static str {
    if cfg!(windows) { "npm.cmd" } else { "npm" }
}

fn wait_for_loopback_server(
    stdout: impl Read + Send + 'static,
    stderr: impl Read + Send + 'static,
) -> Option<String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    drain_preview_output(stdout, sender.clone());
    drain_preview_output(stderr, sender);
    let deadline = Instant::now() + STARTUP_WAIT;
    let origin = receiver.recv_timeout(STARTUP_WAIT).ok()?;
    while Instant::now() < deadline {
        if health::get(Some(&origin)) {
            return Some(origin);
        }
        thread::sleep(Duration::from_millis(50));
    }
    None
}

fn drain_preview_output(mut reader: impl Read + Send + 'static, sender: mpsc::SyncSender<String>) {
    thread::spawn(move || {
        let mut bytes = [0_u8; 1024];
        let mut prefix = String::new();
        let mut notified = false;
        while let Ok(read) = reader.read(&mut bytes) {
            if read == 0 {
                break;
            }
            prefix.push_str(&String::from_utf8_lossy(&bytes[..read]));
            if prefix.len() > MAX_STARTUP_OUTPUT_BYTES {
                prefix.drain(..prefix.len() - MAX_STARTUP_OUTPUT_BYTES);
            }
            if !notified {
                if let Some(origin) = parse_loopback_origin_from_output(&prefix) {
                    let _ = sender.send(origin);
                    notified = true;
                }
            }
        }
    });
}

fn parse_loopback_origin_from_output(output: &str) -> Option<String> {
    for token in output.split(|character: char| {
        character.is_whitespace() || matches!(character, '\'' | '\"' | '(' | ')' | ',')
    }) {
        let start = token.find("http").unwrap_or(token.len());
        let candidate = token.get(start..)?;
        if let Ok(url) = parse_allowed_preview_url(candidate.trim_end_matches(['/', '.', ':'])) {
            return Some(normalized_origin(&url));
        }
    }
    None
}
