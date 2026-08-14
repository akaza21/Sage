use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use uuid::Uuid;

use crate::{acp::limits::LIMITS, project_registry::ProjectRootAuthority};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryPointKind {
    NativeExecutable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequiredContainment {
    EnforcedTree,
    TrustedCooperativeTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActualContainment {
    EnforcedTree,
    TrustedCooperativeTree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchError {
    ArgumentLimit,
    CapacityUnavailable,
    ContainmentFailure,
    Deadline,
    EnvironmentLimit,
    IdentityMismatch,
    InvalidAbsolutePath,
    InvalidArgument,
    InvalidEnvironment,
    Io,
    PlatformUnavailable,
    StdioLimit,
}

pub(crate) struct ArtifactExpectation {
    pub(crate) artifact_id: Uuid,
    pub(crate) canonical_absolute_path: PathBuf,
    pub(crate) sha256: [u8; 32],
    pub(crate) byte_len: u64,
    pub(crate) entrypoint_kind: EntryPointKind,
}

impl ArtifactExpectation {
    pub(crate) fn new(
        artifact_id: Uuid,
        canonical_absolute_path: PathBuf,
        sha256: [u8; 32],
        byte_len: u64,
    ) -> Result<Self, LaunchError> {
        validate_absolute_normalized(&canonical_absolute_path)?;
        Ok(Self {
            artifact_id,
            canonical_absolute_path,
            sha256,
            byte_len,
            entrypoint_kind: EntryPointKind::NativeExecutable,
        })
    }
}

pub(crate) struct BoundedOsArgs(Vec<OsString>);

impl BoundedOsArgs {
    pub(crate) fn new(values: Vec<OsString>) -> Result<Self, LaunchError> {
        if values.len() as u64 > LIMITS.process.arguments {
            return Err(LaunchError::ArgumentLimit);
        }
        let mut total = 0_u64;
        for value in &values {
            let value = value.to_str().ok_or(LaunchError::InvalidArgument)?;
            if value.contains('\0') {
                return Err(LaunchError::InvalidArgument);
            }
            // Curated launch metadata originates as Unicode JSON. Freeze its
            // UTF-8 bytes before native conversion so identical manifests
            // have identical limits on Windows, macOS, and Linux. Native
            // argv storage remains mechanically bounded by this ceiling.
            let bytes = utf8_bytes(value);
            if bytes > LIMITS.process.argument_bytes_each {
                return Err(LaunchError::ArgumentLimit);
            }
            total = total.checked_add(bytes).ok_or(LaunchError::ArgumentLimit)?;
        }
        if total > LIMITS.process.argument_bytes_total {
            return Err(LaunchError::ArgumentLimit);
        }
        Ok(Self(values))
    }

    pub(crate) fn as_slice(&self) -> &[OsString] {
        &self.0
    }
}

pub(crate) struct BoundedEnvironment(Vec<(OsString, OsString)>);

impl BoundedEnvironment {
    pub(crate) fn new(values: Vec<(OsString, OsString)>) -> Result<Self, LaunchError> {
        if values.len() as u64 > LIMITS.process.environment_entries {
            return Err(LaunchError::EnvironmentLimit);
        }
        let mut total = 0_u64;
        let mut keys = HashSet::with_capacity(values.len());
        for (key, value) in &values {
            let key = key.to_str().ok_or(LaunchError::InvalidEnvironment)?;
            let value = value.to_str().ok_or(LaunchError::InvalidEnvironment)?;
            if key.is_empty() || key.contains('\0') || value.contains('\0') || key.contains('=') {
                return Err(LaunchError::InvalidEnvironment);
            }
            if !keys.insert(key.to_lowercase()) {
                return Err(LaunchError::InvalidEnvironment);
            }
            let key_bytes = utf8_bytes(key);
            let value_bytes = utf8_bytes(value);
            if key_bytes > LIMITS.process.environment_key_bytes
                || value_bytes > LIMITS.process.environment_value_bytes
            {
                return Err(LaunchError::EnvironmentLimit);
            }
            total = total
                .checked_add(key_bytes)
                .and_then(|sum| sum.checked_add(value_bytes))
                .ok_or(LaunchError::EnvironmentLimit)?;
        }
        if total > LIMITS.process.environment_bytes_total {
            return Err(LaunchError::EnvironmentLimit);
        }
        Ok(Self(values))
    }

    pub(crate) fn as_slice(&self) -> &[(OsString, OsString)] {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TreeLimits {
    pub(crate) descendants: u32,
    pub(crate) active_processes: u32,
}

impl TreeLimits {
    pub(crate) fn ade_0006a() -> Self {
        Self {
            descendants: u32::try_from(LIMITS.process.descendants_per_tree).unwrap_or(u32::MAX),
            active_processes: u32::try_from(LIMITS.process.windows_job_active_process_limit)
                .unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProcessDeadlines {
    pub(crate) spawn_ms: u64,
    pub(crate) terminate_ms: u64,
}

impl ProcessDeadlines {
    pub(crate) fn ade_0006a() -> Self {
        Self {
            spawn_ms: LIMITS.deadlines_ms.spawn,
            terminate_ms: LIMITS.deadlines_ms.tree_termination,
        }
    }
}

pub(crate) struct LaunchSpec {
    pub(crate) binding_id: Uuid,
    pub(crate) runtime_epoch: u64,
    pub(crate) artifact: ArtifactExpectation,
    pub(crate) fixed_args: BoundedOsArgs,
    pub(crate) environment: BoundedEnvironment,
    pub(crate) project_root: ProjectRootAuthority,
    pub(crate) required_guarantee: RequiredContainment,
    pub(crate) limits: TreeLimits,
    pub(crate) deadlines: ProcessDeadlines,
    pub(crate) absolute_cleanup_deadline: Option<Instant>,
    #[cfg(all(test, windows))]
    pub(crate) test_after_artifact_verified: Option<Box<dyn Fn() + Send + Sync>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeState {
    Starting,
    Running,
    Terminating,
    Exited,
    ContainmentLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeExit {
    Exited { code: Option<i32> },
    Cancelled,
    Deadline,
    IdentityMismatch,
    ContainmentFailure,
    ProtocolFailure,
    ParentShutdown,
}

/// Linear evidence minted only after the native supervisor has observed zero
/// processes for the exact runtime epoch. It is intentionally neither Clone
/// nor constructible outside the process boundary.
#[derive(Debug)]
pub(crate) struct ZeroProcessToken {
    runtime_epoch: u64,
}

impl ZeroProcessToken {
    pub(super) fn new(runtime_epoch: u64) -> Self {
        Self { runtime_epoch }
    }

    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }

    #[cfg(test)]
    pub(crate) fn for_test(runtime_epoch: u64) -> Self {
        Self { runtime_epoch }
    }
}

/// Small platform-neutral launch state used to prevent early or duplicate
/// zero-process proofs. Native resources remain owned by the platform layer.
pub(super) struct LaunchFailureTracker {
    runtime_epoch: u64,
    child_created: bool,
    zero_confirmed: bool,
    token_minted: bool,
}

impl LaunchFailureTracker {
    pub(super) fn pre_spawn(runtime_epoch: u64) -> Self {
        Self {
            runtime_epoch,
            child_created: false,
            zero_confirmed: true,
            token_minted: false,
        }
    }

    pub(super) fn child_created(runtime_epoch: u64) -> Self {
        Self {
            runtime_epoch,
            child_created: true,
            zero_confirmed: false,
            token_minted: false,
        }
    }

    pub(super) fn confirm_zero(&mut self) {
        self.zero_confirmed = true;
    }

    pub(super) fn finish_zero(
        &mut self,
        convergence: Result<(), LaunchError>,
    ) -> Result<ZeroProcessToken, LaunchError> {
        convergence?;
        self.confirm_zero();
        self.take_zero_token()
            .ok_or(LaunchError::ContainmentFailure)
    }

    pub(super) fn take_zero_token(&mut self) -> Option<ZeroProcessToken> {
        if !self.zero_confirmed || self.token_minted {
            return None;
        }
        self.token_minted = true;
        Some(ZeroProcessToken::new(self.runtime_epoch))
    }

    pub(super) fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }

    #[cfg(test)]
    pub(super) fn child_was_created(&self) -> bool {
        self.child_created
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeardownAction {
    CloseStdin,
    TerminateTree,
    AwaitChildExit,
    AwaitZeroProcesses,
    CloseNativeHandles,
    ReleaseReservation,
}

pub(crate) struct SupervisorLifecycle {
    state: TreeState,
    requested_exit: Option<TreeExit>,
    observed_exit: Option<TreeExit>,
    root_alive: bool,
    descendants: u32,
    descendant_limit: u32,
    reservation_owned: bool,
}

impl SupervisorLifecycle {
    pub(crate) fn starting(descendant_limit: u32) -> Self {
        Self {
            state: TreeState::Starting,
            requested_exit: None,
            observed_exit: None,
            root_alive: true,
            descendants: 0,
            descendant_limit,
            reservation_owned: true,
        }
    }

    pub(crate) fn mark_running(&mut self) -> bool {
        if self.state != TreeState::Starting {
            return false;
        }
        self.state = TreeState::Running;
        true
    }

    pub(crate) fn request_termination(&mut self, reason: TreeExit) -> bool {
        if !matches!(self.state, TreeState::Starting | TreeState::Running) {
            return false;
        }
        self.requested_exit = Some(reason);
        self.state = TreeState::Terminating;
        true
    }

    pub(crate) fn observe_processes(&mut self, root_alive: bool, descendants: u32) -> TreeState {
        self.root_alive = root_alive;
        self.descendants = descendants;
        if descendants > self.descendant_limit {
            self.state = TreeState::ContainmentLost;
        } else if !root_alive && descendants == 0 {
            self.state = TreeState::Exited;
            self.observed_exit = Some(
                self.requested_exit
                    .unwrap_or(TreeExit::Exited { code: None }),
            );
        }
        self.state
    }

    pub(crate) fn mark_containment_deadline(&mut self) {
        if self.root_alive || self.descendants != 0 {
            self.state = TreeState::ContainmentLost;
        }
    }

    pub(crate) fn release_reservation(&mut self) -> bool {
        if self.state != TreeState::Exited || !self.reservation_owned {
            return false;
        }
        self.reservation_owned = false;
        true
    }

    pub(crate) fn state(&self) -> TreeState {
        self.state
    }

    pub(crate) fn exit(&self) -> Option<TreeExit> {
        self.observed_exit
    }

    pub(crate) fn teardown_plan(&self) -> [TeardownAction; 6] {
        [
            TeardownAction::CloseStdin,
            TeardownAction::TerminateTree,
            TeardownAction::AwaitChildExit,
            TeardownAction::AwaitZeroProcesses,
            TeardownAction::CloseNativeHandles,
            TeardownAction::ReleaseReservation,
        ]
    }
}

fn validate_absolute_normalized(path: &Path) -> Result<(), LaunchError> {
    if !path.is_absolute()
        || path.components().any(|part| match part {
            Component::CurDir | Component::ParentDir => true,
            // Windows verbatim paths intentionally disable ordinary
            // component normalization, so `.` and `..` may arrive as
            // Normal rather than CurDir/ParentDir.
            Component::Normal(value) => matches!(value.to_str(), Some(".") | Some("..")),
            Component::Prefix(_) | Component::RootDir => false,
        })
    {
        return Err(LaunchError::InvalidAbsolutePath);
    }
    Ok(())
}

fn utf8_bytes(value: &str) -> u64 {
    u64::try_from(value.len()).unwrap_or(u64::MAX)
}
