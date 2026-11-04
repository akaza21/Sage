use crate::acp::browser::attachment::{MAX_AGENT_TABS_PER_SESSION, MAX_POPUP_TABS_PER_SESSION};
use crate::acp::browser::{BrowserFailure, BrowserScope, attachment::AttachmentLease};

const MAX_LANES: usize = 1 + MAX_AGENT_TABS_PER_SESSION + MAX_POPUP_TABS_PER_SESSION;

pub(crate) struct AttachmentLanes<Port> {
    entries: Vec<(BrowserScope, AttachmentLease, u64, Port)>,
}

impl<Port> Default for AttachmentLanes<Port> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<Port> AttachmentLanes<Port> {
    pub(crate) fn get_or_try_insert(
        &mut self,
        scope: &BrowserScope,
        lease: &AttachmentLease,
        create: impl FnOnce() -> Result<Port, BrowserFailure>,
    ) -> Result<&mut Port, BrowserFailure> {
        self.entries.retain(|(cached_scope, cached, generation, _)| {
            cached.is_live()
                && cached.resource_generation() == *generation
                && (cached_scope.tab_id != scope.tab_id
                    || (cached_scope == scope && cached.same_attachment(lease)))
        });
        let index = match self.entries.iter().position(|(cached_scope, cached, _, _)| {
            cached_scope == scope && cached.same_attachment(lease)
        }) {
            Some(index) => index,
            None => {
                if self.entries.len() >= MAX_LANES {
                    return Err(BrowserFailure::Capacity);
                }
                let generation = lease.resource_generation();
                let port = create()?;
                self.entries.push((scope.clone(), lease.clone(), generation, port));
                self.entries.len() - 1
            }
        };
        Ok(&mut self.entries[index].3)
    }
}

#[cfg(test)]
mod tests;
