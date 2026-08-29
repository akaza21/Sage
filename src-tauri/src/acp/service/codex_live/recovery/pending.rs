use std::sync::Arc;

use crate::acp::{
    process::TreeControl,
    runtime::{CoordinatorActivationJournal, LiveRunnerHandle, RuntimeHost, SessionTreeLease},
    scheduler::RuntimeOwner,
    store::ActivationAbortCategory,
};

use super::super::{ActivationAbortIntent, CodexActivationDriver, live_recovery::LiveHostRecovery};

// Each variant is a unique, non-cloneable recovery authority. Keeping its
// payload inline makes replacement under the single mutex atomic.
#[allow(clippy::large_enum_variant)]
pub(in crate::acp::service) enum PendingCodexRecovery {
    Activation {
        recovery: crate::acp::runtime::ActivationRecovery,
    },
    Launch {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        recovery: crate::acp::process::LaunchRecovery,
    },
    PreRunner {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        control: TreeControl,
    },
    ZeroToken {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        token: crate::acp::process::ZeroProcessToken,
    },
    Abort {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        proof: crate::acp::runtime::ZeroProcessProof,
    },
    ActivationCommitted {
        owner: RuntimeOwner,
    },
    /// The activation journal could not be converted to a narrower lease.
    /// It remains the only authority that can decide whether the durable
    /// session is still activating or has entered the runtime phase.
    Journal {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        journal: CoordinatorActivationJournal,
        proof: crate::acp::process::ZeroProcessToken,
        host: Option<(Arc<RuntimeHost>, bool)>,
    },
    /// A constructed host has closed its mailbox but did not join before the
    /// activation cleanup deadline.  Keep the exact host until the join and
    /// matching slot release both converge.
    StartupHost {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        claimed: bool,
    },
    /// A live command reached an effect-unknown outcome.  The slot and this
    /// payload intentionally retain the same host until bounded shutdown (or
    /// the close recovery path) proves it is safe to release capacity.
    LiveHost(LiveHostRecovery),
    Driver {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        driver: CodexActivationDriver,
    },
    Projection {
        owner: RuntimeOwner,
        lease: SessionTreeLease,
        proof: crate::acp::runtime::ZeroProcessProof,
        host: Arc<RuntimeHost>,
    },
    ProjectionToken {
        owner: RuntimeOwner,
        lease: SessionTreeLease,
        token: crate::acp::process::ZeroProcessToken,
        host: Arc<RuntimeHost>,
    },
    Runner {
        owner: RuntimeOwner,
        runner: Arc<LiveRunnerHandle>,
    },
    RuntimeZeroToken {
        owner: RuntimeOwner,
        token: crate::acp::process::ZeroProcessToken,
    },
    RuntimeProjection {
        owner: RuntimeOwner,
        proof: crate::acp::runtime::ZeroProcessProof,
    },
}
