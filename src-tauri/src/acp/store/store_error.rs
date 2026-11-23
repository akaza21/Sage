//! Why a store mutation was refused or failed. Callers map each variant to
//! their own outcome (an IPC error, a declined harness turn, recovery).

#[derive(Debug, thiserror::Error)]
pub(crate) enum StoreError {
    #[error("{reason}")]
    RewindUnavailable { reason: &'static str },
    #[error("database operation failed")]
    Database(#[from] rusqlite::Error),
    #[error("durable history capacity reached for {resource}; existing history is preserved")]
    Capacity { resource: &'static str, limit: u64 },
    #[error("stale or foreign {resource} intent")]
    Conflict { resource: &'static str },
    #[error("invalid durable transition for {resource}")]
    InvalidTransition { resource: &'static str },
    #[error("conflicting terminal update for {resource}")]
    TerminalConflict { resource: &'static str },
    #[error("durable data is corrupt: {0}")]
    Corrupt(&'static str),
    #[error("normalized payload exceeds the frozen limit")]
    PayloadTooLarge,
    #[error("journal transaction shape is unsupported")]
    BatchShape,
    #[error("SQLite commit outcome is unknown; authoritative bootstrap is required")]
    CommitUnknown,
    #[error("a monotonic SQLite counter is exhausted")]
    CounterExhausted,
    /// Sage holds a repository lease (a branch switch) over this Project.
    #[error("{0}")]
    RepositoryBusy(crate::repository_mutation_guard::RepositoryPause),
}
