//! Curated bridge authority. Retains at most one 512 KiB old/new pair or one
//! shell command awaiting approval.
pub(crate) mod files;
pub(crate) mod install;
pub(crate) mod path;
mod refusal;
pub(crate) mod write;
use crate::{settings_store::SessionAccessMode, workspace::Workspace};
#[cfg(any(test, feature = "acp-fixture"))]
pub(crate) use refusal::MAX_REPLY_BYTES;
pub(crate) use refusal::Refusal;

pub(crate) struct Bridge {
    pub workspace: Workspace,
    pub mode: SessionAccessMode,
    pub pending: Option<Pending>,
    pub writes: u8,
    pub used: std::collections::BTreeSet<String>,
    /// The last command whose approval reached Pi. Pi may have expired the
    /// dialog first and ask again by withdrawing; the approval still stands.
    pub approved_shell: Option<String>,
}
pub(crate) struct Pending {
    pub request_id: String,
    pub prepared: files::Prepared,
    pub approved: bool,
    pub written: bool,
}
impl Bridge {
    pub(crate) fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            mode: SessionAccessMode::ReadOnly,
            pending: None,
            writes: 0,
            used: std::collections::BTreeSet::new(),
            approved_shell: None,
        }
    }
    pub(crate) fn begin_turn(&mut self, mode: SessionAccessMode) {
        self.mode = mode;
        self.pending = None;
        self.writes = 0;
        self.used.clear();
        self.approved_shell = None;
    }
}

#[cfg(test)]
mod tests;
