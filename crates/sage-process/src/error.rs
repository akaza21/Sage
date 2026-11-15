//! Why launching or containing a child process failed.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
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
