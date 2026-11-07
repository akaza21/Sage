//! Curated bridge authority. Retains at most one 512 KiB old/new pair.
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
}
pub(crate) struct Pending {
    pub request_id: String,
    pub edit: files::Edit,
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
        }
    }
    pub(crate) fn begin_turn(&mut self, mode: SessionAccessMode) {
        self.mode = mode;
        self.pending = None;
        self.writes = 0;
        self.used.clear();
    }
}

#[cfg(test)]
mod tests;
