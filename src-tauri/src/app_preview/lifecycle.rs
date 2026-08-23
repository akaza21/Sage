use std::{
    collections::BTreeMap,
    io::Read,
    process::{Child, Command, Stdio},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use super::{
    discovery::{PreviewDiscoveryRequest, PreviewTargetRef},
    error::PreviewError,
    health,
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
struct Registry {
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

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

pub(crate) fn replace_discovered(
    request: &PreviewDiscoveryRequest,
    targets: Vec<(PreviewTargetRef, std::path::PathBuf)>,
) {
    let mut registry = registry().lock().expect("preview lifecycle registry");
    registry.discovered.retain(|target, _| {
        target.project_id == request.project_id
            && target.expected_transition_epoch == request.expected_transition_epoch
    });
    registry.discovered.clear();
    registry.discovered.extend(targets);
}

pub(crate) fn start(target: PreviewTargetRef) -> Result<PreviewStartResponse, PreviewError> {
    let (stdout, stderr) = {
        let mut registry = registry().lock().expect("preview lifecycle registry");
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
        (stdout, stderr)
    };
    let origin = wait_for_loopback_server(stdout, stderr);
    let Some(origin) = origin else {
        stop_starting(&target);
        return Err(PreviewError::Unavailable {
            message: "Preview script did not expose a healthy loopback server in time.".into(),
        });
    };
    let promoted = {
        let mut registry = registry().lock().expect("preview lifecycle registry");
        let still_current = registry
            .starting
            .as_ref()
            .is_some_and(|starting| starting.target == target)
            && registry.discovered.contains_key(&target)
            && registry.active.is_none();
        if !still_current {
            false
        } else {
            let StartingPreview { child, .. } = registry.starting.take().expect("starting preview");
            registry.active = Some(ActivePreview {
                target: target.clone(),
                origin: origin.clone(),
                child,
            });
            true
        }
    };
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
    let mut registry = registry().lock().expect("preview lifecycle registry");
    let active = registry.active.as_mut()?;
    if active.target != *target {
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
}

pub(crate) fn stop(target: &PreviewTargetRef) -> Result<PreviewStopResponse, PreviewError> {
    let active = {
        let mut registry = registry().lock().expect("preview lifecycle registry");
        match registry.active.take() {
            Some(active) if active.target == *target => active,
            Some(active) => {
                registry.active = Some(active);
                return Err(PreviewError::conflict(
                    "previewTarget",
                    "Preview target is not active.",
                ));
            }
            None => {
                return Err(PreviewError::conflict(
                    "activePreview",
                    "No Preview target is active.",
                ));
            }
        }
    };
    let mut active = active;
    stop_child(&mut active.child);
    Ok(PreviewStopResponse {
        ref_: target.clone(),
    })
}

pub(crate) fn stop_before_epoch(new_epoch: u64) {
    let active = {
        let mut registry = registry().lock().expect("preview lifecycle registry");
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
    };
    if let Some(mut active) = active {
        stop_child(&mut active.child);
    }
}

fn stop_starting(target: &PreviewTargetRef) {
    let mut registry = registry().lock().expect("preview lifecycle registry");
    if registry
        .starting
        .as_ref()
        .is_some_and(|starting| starting.target == *target)
    {
        let starting = registry.starting.as_mut().expect("starting preview");
        stop_child(&mut starting.child);
        registry.starting = None;
    }
}

struct ManagedChild {
    child: Child,
    #[cfg(windows)]
    job: PreviewJob,
}

impl ManagedChild {
    fn reserve(child: Child) -> Result<Self, Child> {
        #[cfg(windows)]
        {
            match PreviewJob::new(&child) {
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

#[cfg(windows)]
struct PreviewJob(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: The HANDLE is an owned Windows job object used only while the registry mutex is held.
// The OS handle may be signaled from any thread, and we never share the raw pointer without
// that lock.
#[cfg(windows)]
unsafe impl Send for PreviewJob {}

#[cfg(windows)]
impl PreviewJob {
    fn new(child: &Child) -> Result<Self, ()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
        };

        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == std::ptr::null_mut() {
            return Err(());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) != 0
        };
        let assigned =
            configured && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) != 0 };
        if assigned {
            Ok(Self(job))
        } else {
            unsafe {
                TerminateJobObject(job, 1);
                CloseHandle(job);
            }
            Err(())
        }
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;

        unsafe { TerminateJobObject(self.0, 1) };
    }
}

#[cfg(windows)]
impl Drop for PreviewJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;

        unsafe { CloseHandle(self.0) };
    }
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
