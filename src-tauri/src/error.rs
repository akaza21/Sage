#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitContainmentState {
    TerminatedUnreaped,
    PotentiallyRunning,
}

/// A repository layout Sage refuses because Git would read repository data
/// from outside the opened folder, where Sage holds no handle on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryLayout {
    /// The opened folder is below the repository's top level.
    NestedRoot,
    /// The repository's Git data lives outside the opened folder, as in a
    /// linked worktree or a `gitdir:` file pointing elsewhere.
    ExternalGitDir,
}

impl RepositoryLayout {
    pub fn public_message(self) -> &'static str {
        match self {
            Self::NestedRoot => {
                "This folder is inside a parent Git repository. Open the repository root to use Git."
            }
            Self::ExternalGitDir => {
                "This folder's Git data is stored outside it, as in a linked worktree. Open the repository root to use Git."
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("database operation failed: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("unknown harness: {0}")]
    UnknownHarness(String),

    #[error("harness is not available: {0}")]
    HarnessUnavailable(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[cfg_attr(windows, allow(dead_code))]
    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),

    #[error("Git operation failed: {0}")]
    Git(String),

    /// Sage refused to run Git because the repository's own config names a
    /// program Git would run. The message is safe to show to the user.
    #[error("{0}")]
    UnsafeRepositoryConfig(String),

    #[error("{}", .0.public_message())]
    RepositoryOutsideRoot(RepositoryLayout),

    #[error("Git mutation effect is unknown: {0}")]
    GitEffectUnknown(String),

    #[error("Git process containment is active: {message}")]
    GitContainment {
        state: GitContainmentState,
        message: String,
    },
}

pub type AppResult<T> = Result<T, AppError>;
