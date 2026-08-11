// P1 defines the private process contract before P2 connects the runtime.
// Keep the dormant private surface warning-clean without making it public.
#![allow(dead_code, unused_imports)]

mod capacity;
mod identity;
mod platform;
mod supervisor;
#[cfg(not(windows))]
mod unavailable;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::monotonic_tick;

pub(crate) use capacity::{TreeCapacityLease, TreeCapacityPool};
pub(crate) use identity::{ProcessIdentityReceipt, SpawnTimingReceipt};
pub(crate) use platform::{
    LaunchFailure, LaunchRecovery, PlatformSupervisor, RunningTree, StdinEndpoint, StdoutEndpoint,
    TreeControl,
};
pub(crate) use supervisor::ZeroProcessToken;
pub(crate) use supervisor::{
    ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, LaunchError, LaunchSpec,
    ProcessDeadlines, RequiredContainment, TreeLimits,
};
pub(crate) use supervisor::{SupervisorLifecycle, TeardownAction, TreeExit, TreeState};

#[cfg(test)]
mod tests;
