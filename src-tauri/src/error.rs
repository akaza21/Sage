#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitContainmentState {
    TerminatedUnreaped,
    PotentiallyRunning,
}

/// A repository layout Sage refuses because Git would reach repository data
/// other than the opened folder's own `.git` directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepositoryLayout {
    /// The opened folder is below the repository's top level.
    NestedRoot,
    /// The repository's Git data is not the opened folder's own `.git`
    /// directory, as with a linked worktree, a `gitdir:` file or a
    /// `commondir` pointer.
    ExternalGitDir,
}

impl RepositoryLayout {
    pub fn public_message(self) -> &'static str {
        match self {
            Self::NestedRoot => {
                "This folder is inside a parent Git repository. Open the repository root to use Git."
            }
            Self::ExternalGitDir => {
                "This folder's Git data is not its own .git directory, as in a linked worktree or submodule. Open the repository root to use Git."
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
