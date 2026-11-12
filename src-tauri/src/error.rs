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

    #[error("Mixed line endings require a read-only viewer")]
    MixedLineEndings,

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

    /// The profile's database was migrated by a newer Sage than this one.
    /// Opening it would mean guessing at a schema this build does not know,
    /// so Sage refuses before changing anything.
    #[error(
        "This Sage profile was last opened by a newer version of Sage (data version \
         {stored_version}; this version reads up to {supported_version}). Sage has left it \
         unchanged. Open it with that newer Sage. Test and development builds should run \
         with their own profile (SAGE_DISPOSABLE_PROFILE_ROOT) so they never upgrade this one."
    )]
    NewerProfile {
        stored_version: i64,
        supported_version: u32,
    },

    /// The background task running an operation ended before reporting an
    /// outcome (it panicked or was cancelled). The detail is for logs only.
    #[error("background task ended before finishing: {0}")]
    TaskInterrupted(String),
}

impl AppError {
    /// The text a person reads in the UI. A typed refusal already carries a
    /// sentence written for them, so it surfaces without the category prefix
    /// that `Display` keeps for logs. An interrupted task or a failed database
    /// operation gets a retry sentence of its own instead of its detail (see
    /// `hides_detail`); every other error reads as `Display` does.
    pub fn public_message(&self) -> String {
        match self {
            Self::InvalidRequest(message) => message.clone(),
            Self::TaskInterrupted(_) => "Sage could not finish this change; try again.".into(),
            Self::Database(_) => "Sage could not save this change; try again.".into(),
            other => other.to_string(),
        }
    }

    /// Whether `public_message` leaves out the error's detail, which the
    /// caller then logs.
    pub fn hides_detail(&self) -> bool {
        matches!(self, Self::TaskInterrupted(_) | Self::Database(_))
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_messages_drop_the_refusal_prefix_and_hide_task_detail() {
        let refusal = AppError::InvalidRequest("Only an archived Chat can be deleted".into());
        assert_eq!(
            refusal.to_string(),
            "invalid request: Only an archived Chat can be deleted"
        );
        assert_eq!(
            refusal.public_message(),
            "Only an archived Chat can be deleted"
        );
        let interrupted = AppError::TaskInterrupted("task 7 panicked".into());
        assert_eq!(
            interrupted.public_message(),
            "Sage could not finish this change; try again."
        );
        assert!(!interrupted.public_message().contains("panicked"));
        let database = AppError::Database(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("database is locked in chat_tabs".into()),
        ));
        assert_eq!(
            database.public_message(),
            "Sage could not save this change; try again."
        );
        assert!(!database.public_message().contains("chat_tabs"));
        assert!(database.hides_detail() && interrupted.hides_detail());
        let limit = AppError::ResourceLimit("too many Chats".into());
        assert_eq!(limit.public_message(), limit.to_string());
        assert!(!limit.hides_detail() && !refusal.hides_detail());
        let layout = AppError::RepositoryOutsideRoot(RepositoryLayout::NestedRoot);
        assert_eq!(
            layout.public_message(),
            RepositoryLayout::NestedRoot.public_message()
        );
    }
}
