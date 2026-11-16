//! Codex lists its models through `model/list` without a signed-in account
//! or a Project, so the installed build is probed the way discovery reads a
//! Project's catalog, from the user's profile folder.

use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use sage_acp_protocol::contract::RuntimeEpoch;
use sage_domain::harness::ModelOption;
#[cfg(unix)]
use sage_process::process_group::ProcessGroup as ProbeTree;
#[cfg(windows)]
use sage_process::process_tree::ProcessTree as ProbeTree;

use super::{
    CodexDiscoveryProtocol,
    runner::{
        CodexDiscoveryDeadline, CodexDiscoveryRunError, DiscoveryTransport, DiscoveryZeroProcess,
        RevalidationCheckpoint, run_discovery_protocol,
    },
};
use crate::acp::{
    bindings::codex::AdmittedCodexBinding,
    ipc::CodexDiscoveryModel,
    transport::{Frame, FrameError, FrameWriteOutcome, OutboundFrame},
};

const PROBE_OPERATION: Duration = Duration::from_secs(20);
const PROBE_CLEANUP: Duration = Duration::from_secs(5);
/// How long a probe whose stdin is closed may take to end its output before
/// it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(2);
/// How often teardown asks whether the probe's processes have ended.
const SETTLE_POLL: Duration = Duration::from_millis(5);
/// Startup output and four catalog pages; this bounds a misbehaving CLI.
const PROBE_BYTES: u64 = 4 * 1024 * 1024;
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Probe roots left to a reaper thread at once; no probe starts while this
/// many are stuck, so a CLI wedged in the kernel cannot leak threads.
const MAX_STUCK_ROOTS: usize = 4;
/// The probe owns no session, so its runtime epoch only pairs its own exit.
const PROBE_EPOCH: u64 = 1;

#[derive(Debug)]
enum Listed {
    Models(Vec<ModelOption>),
    Failed(Instant),
}

#[derive(Debug, Default)]
struct CatalogState {
    /// Advances when the account changes, so a listing read before then is
    /// not stored after it.
    generation: u64,
    /// Advances with every stored listing, so a probe that began before one
    /// does not replace it.
    revision: u64,
    listed: Option<([u8; 32], Listed)>,
}

impl CatalogState {
    fn store(&mut self, binding_digest: [u8; 32], listed: Listed) {
        self.revision = self.revision.wrapping_add(1);
        self.listed = Some((binding_digest, listed));
    }

    fn models(&self, binding_digest: [u8; 32]) -> Option<Vec<ModelOption>> {
        match &self.listed {
            Some((digest, Listed::Models(models))) if *digest == binding_digest => {
                Some(models.clone())
            }
            _ => None,
        }
    }
}

/// The models the admitted Codex build lists before any session, by binding
/// digest: from a probe, or refreshed by a signed-in Project discovery.
#[derive(Debug, Default)]
pub(crate) struct CodexModelCatalog {
    state: Mutex<CatalogState>,
    /// Held while a probe runs, so concurrent lookups share its outcome.
    probing: Mutex<()>,
    /// Killed probe roots a reaper thread is still waiting for.
    stuck_roots: Arc<AtomicUsize>,
}

impl CodexModelCatalog {
    pub(crate) fn known(&self, binding_digest: [u8; 32]) -> Option<Vec<ModelOption>> {
        lock(&self.state).models(binding_digest)
    }

    /// `None` when the build could not be read; a later lookup probes again
    /// once [`RETRY_AFTER`] has passed.
    pub(crate) fn probed(&self, binding: &AdmittedCodexBinding) -> Option<Vec<ModelOption>> {
        self.probed_within(binding, PROBE_OPERATION, PROBE_CLEANUP)
    }

    fn probed_within(
        &self,
        binding: &AdmittedCodexBinding,
        operation: Duration,
        cleanup: Duration,
    ) -> Option<Vec<ModelOption>> {
        let digest = binding.binding_digest();
        if let Some(known) = self.settled(digest) {
            return known;
        }
        let _probing = lock(&self.probing);
        if let Some(known) = self.settled(digest) {
            return known;
        }
        let (generation, revision) = {
            let state = lock(&self.state);
            (state.generation, state.revision)
        };
        let models = CodexDiscoveryDeadline::starting_now(operation, cleanup)
            .ok()
            .and_then(|deadline| probe(binding, deadline, &self.stuck_roots))
            .filter(|models| !models.is_empty());
        let mut state = lock(&self.state);
        if state.generation != generation {
            return None;
        }
        if state.revision != revision {
            return state.models(digest);
        }
        let listed = models
            .clone()
            .map_or_else(|| Listed::Failed(Instant::now()), Listed::Models);
        state.store(digest, listed);
        models
    }

    fn settled(&self, binding_digest: [u8; 32]) -> Option<Option<Vec<ModelOption>>> {
        match &lock(&self.state).listed {
            Some((digest, Listed::Models(models))) if *digest == binding_digest => {
                Some(Some(models.clone()))
            }
            Some((digest, Listed::Failed(at)))
                if *digest == binding_digest && at.elapsed() < RETRY_AFTER =>
            {
                Some(None)
            }
            _ => None,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        lock(&self.state).generation
    }

    /// Stores what a signed-in discovery listed, unless the account changed
    /// after `generation` was read.
    pub(crate) fn refresh(
        &self,
        generation: u64,
        binding_digest: [u8; 32],
        models: Vec<ModelOption>,
    ) -> bool {
        let mut state = lock(&self.state);
        if state.generation != generation {
            return false;
        }
        state.store(binding_digest, Listed::Models(models));
        true
    }

    /// The account changed: drop its listing and refuse any read before now.
    pub(crate) fn forget(&self) {
        let mut state = lock(&self.state);
        state.generation = state.generation.wrapping_add(1);
        state.listed = None;
    }
}

/// Only a build the user installed is probed: Sage's managed package runs
/// only inside its supervised containment.
fn probe(
    binding: &AdmittedCodexBinding,
    deadline: CodexDiscoveryDeadline,
    stuck_roots: &Arc<AtomicUsize>,
) -> Option<Vec<ModelOption>> {
    if !binding.is_observed() || binding.is_desktop_fixture() {
        return None;
    }
    let stuck = stuck_roots.load(Ordering::Acquire);
    if stuck >= MAX_STUCK_ROOTS {
        tracing::warn!(
            stuck_roots = stuck,
            "Codex model probe skipped while earlier probe processes are still exiting"
        );
        return None;
    }
    binding.revalidate().ok()?;
    let environment: Vec<(OsString, OsString)> = binding
        .launch_environment()
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
    let mut command = Command::new(&binding.entrypoint().path);
    command
        .arg("app-server")
        .env_clear()
        .envs(environment.iter().cloned())
        .current_dir(profile_directory(&environment)?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    isolate(&mut command);
    let mut transport = ProbeTransport::spawn(
        command,
        binding,
        deadline.cleanup_end(),
        Arc::clone(stuck_roots),
    )?;
    let runtime_epoch = RuntimeEpoch::new(PROBE_EPOCH).ok()?;
    let mut protocol = CodexDiscoveryProtocol::from_admitted_binding(binding, runtime_epoch);
    run_discovery_protocol(&mut protocol, &mut transport, PROBE_EPOCH, deadline)
        .ok()
        .map(|content| {
            content
                .models
                .iter()
                .map(|model| (&CodexDiscoveryModel::from(model)).into())
                .collect()
        })
}

fn profile_directory(environment: &[(OsString, OsString)]) -> Option<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    environment
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| PathBuf::from(value))
        .filter(|path| path.is_absolute() && path.is_dir())
}

/// Everything the probe starts, such as the node and codex.exe behind a
/// `.cmd` shim, runs inside its tree: a Job Object on Windows, the process
/// group it leads on Unix.
fn isolate(command: &mut Command) {
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

#[cfg(unix)]
fn contain(child: &Child) -> io::Result<ProbeTree> {
    ProbeTree::led_by(child.id())
}

#[cfg(windows)]
fn contain(child: &Child) -> io::Result<ProbeTree> {
    ProbeTree::contain_blocking(child)
}

struct Running {
    child: Child,
    tree: ProbeTree,
}

struct ProbeTransport<'a> {
    /// Taken when the probe is torn down.
    running: Option<Running>,
    stdin: Option<ChildStdin>,
    frames: mpsc::Receiver<Result<Option<Frame>, FrameError>>,
    reader: Option<JoinHandle<()>>,
    binding: &'a AdmittedCodexBinding,
    /// The probe's teardown ends by then, whichever path tears it down.
    cleanup_end: Instant,
    /// How teardown learns the tree is empty; a test stands in a tree that
    /// never empties, since no real process outlives the tree's kill.
    tree_empty: fn(&ProbeTree) -> bool,
    stuck_roots: Arc<AtomicUsize>,
}

impl<'a> ProbeTransport<'a> {
    fn spawn(
        mut command: Command,
        binding: &'a AdmittedCodexBinding,
        cleanup_end: Instant,
        stuck_roots: Arc<AtomicUsize>,
    ) -> Option<Self> {
        let mut child = command.spawn().ok()?;
        let tree = match contain(&child) {
            Ok(tree) => tree,
            Err(_) => {
                let _ = child.kill();
                reap_root(child, cleanup_end, &stuck_roots);
                return None;
            }
        };
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let (sender, frames) = mpsc::sync_channel(4);
        let mut transport = Self {
            running: Some(Running { child, tree }),
            stdin,
            frames,
            reader: None,
            binding,
            cleanup_end,
            tree_empty: ProbeTree::is_empty,
            stuck_roots,
        };
        let stdout = stdout?;
        transport.reader = Some(thread::spawn(move || {
            frames::read_frames(stdout.take(PROBE_BYTES), &sender);
        }));
        Some(transport)
    }

    /// Codex exits once its stdin closes. Whatever is left of its tree after
    /// the grace, such as a child still holding its stdout, is killed before
    /// the root is reaped, while the root still pins the tree's identity.
    /// Zero processes are proven only once the root is reaped and the whole
    /// tree is empty by `cleanup_end`; teardown never waits past it.
    fn tear_down(&mut self, cleanup_end: Instant) -> Result<(), CodexDiscoveryRunError> {
        let Some(Running { mut child, tree }) = self.running.take() else {
            return Ok(());
        };
        self.stdin = None;
        let now = Instant::now();
        self.drain_output(now + EXIT_GRACE.min(cleanup_end.saturating_duration_since(now)));
        let tree_signalled = tree.terminate();
        let root_signalled = child.kill().is_ok();
        let zero = reap_root(child, cleanup_end, &self.stuck_roots)
            && settles_by(cleanup_end, || (self.tree_empty)(&tree));
        if self.drain_output(cleanup_end)
            && let Some(reader) = self.reader.take()
        {
            let _ = reader.join();
        }
        (tree_signalled && root_signalled && zero)
            .then_some(())
            .ok_or(CodexDiscoveryRunError::Cleanup)
    }

    /// Discards output until the reader ends at end of stream; false when
    /// `until` passes first. Draining keeps the reader from blocking on a
    /// full channel.
    fn drain_output(&self, until: Instant) -> bool {
        loop {
            match self
                .frames
                .recv_timeout(until.saturating_duration_since(Instant::now()))
            {
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return true,
                Err(mpsc::RecvTimeoutError::Timeout) => return false,
            }
        }
    }
}

/// Reaps a killed root by `until`; a root that has not ended by then, such as
/// one stuck in uninterruptible I/O, is left to a thread that reaps it later
/// and is counted in `stuck_roots` until it does.
fn reap_root(mut child: Child, until: Instant, stuck_roots: &Arc<AtomicUsize>) -> bool {
    if settles_by(until, || matches!(child.try_wait(), Ok(Some(_)))) {
        return true;
    }
    stuck_roots.fetch_add(1, Ordering::AcqRel);
    let reaped = Arc::clone(stuck_roots);
    let reaper = thread::Builder::new().spawn(move || {
        let _ = child.wait();
        reaped.fetch_sub(1, Ordering::AcqRel);
    });
    if reaper.is_err() {
        stuck_roots.fetch_sub(1, Ordering::AcqRel);
    }
    false
}

/// Polls `settled` until it holds or `until` passes; true when it held.
fn settles_by(until: Instant, mut settled: impl FnMut() -> bool) -> bool {
    loop {
        if settled() {
            return true;
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        thread::sleep(SETTLE_POLL.min(left));
    }
}

impl DiscoveryTransport for ProbeTransport<'_> {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let Some(stdin) = self.stdin.as_mut() else {
            return FrameWriteOutcome::ZeroByteFailure;
        };
        let written = stdin
            .write_all(frame.payload())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush());
        match written {
            Ok(()) => FrameWriteOutcome::Complete,
            Err(_) => FrameWriteOutcome::ZeroByteFailure,
        }
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        match self.frames.recv_timeout(timeout) {
            Ok(Ok(frame)) => Ok(frame),
            Ok(Err(_)) => Err(CodexDiscoveryRunError::Framing),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(CodexDiscoveryRunError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(CodexDiscoveryRunError::ReaderDisconnected)
            }
        }
    }

    fn revalidate(&self, _: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
        self.binding
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)
    }

    fn close_stdin(&mut self) {
        self.stdin = None;
    }

    fn terminate_zero(
        &mut self,
        _: bool,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        self.tear_down(deadline.cleanup_end())?;
        Ok(DiscoveryZeroProcess::reaped(PROBE_EPOCH))
    }
}

impl Drop for ProbeTransport<'_> {
    fn drop(&mut self) {
        let _ = self.tear_down(self.cleanup_end);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

mod frames;
#[cfg(test)]
mod tests;
