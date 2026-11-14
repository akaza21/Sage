use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use super::contract::{SageChatId, SageProjectId};
use sage_core::limits::LIMITS;

type Owner = (String, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageAdmissionError {
    Owner,
    Global,
}

#[derive(Debug, Default)]
struct State {
    global: u64,
    owners: BTreeMap<Owner, u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PageReadAdmission {
    state: Arc<Mutex<State>>,
}

impl PageReadAdmission {
    pub(crate) fn try_acquire(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
    ) -> Result<PageReadLease, PageAdmissionError> {
        let owner = (project.as_str().to_owned(), chat.as_str().to_owned());
        let mut state = lock(&self.state);
        if state.global >= LIMITS.durable.outstanding_page_reads_global {
            return Err(PageAdmissionError::Global);
        }
        let owner_count = state.owners.get(&owner).copied().unwrap_or(0);
        if owner_count >= LIMITS.durable.outstanding_page_reads_per_session {
            return Err(PageAdmissionError::Owner);
        }
        state.global += 1;
        state.owners.insert(owner.clone(), owner_count + 1);
        Ok(PageReadLease {
            state: Arc::clone(&self.state),
            owner: Some(owner),
        })
    }
}

#[derive(Debug)]
pub(crate) struct PageReadLease {
    state: Arc<Mutex<State>>,
    owner: Option<Owner>,
}

impl Drop for PageReadLease {
    fn drop(&mut self) {
        let Some(owner) = self.owner.take() else {
            return;
        };
        let mut state = lock(&self.state);
        state.global = state.global.saturating_sub(1);
        let Some(count) = state.owners.get_mut(&owner) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            state.owners.remove(&owner);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_id(value: &str) -> SageProjectId {
        SageProjectId::parse(value).unwrap()
    }
    fn chat(value: &str) -> SageChatId {
        SageChatId::parse(value).unwrap()
    }

    #[test]
    fn refuses_before_waiting_at_owner_and_global_limits_then_releases() {
        let admission = PageReadAdmission::default();
        let project = project_id("project");
        let chat_a = chat("chat-a");
        let chat_b = chat("chat-b");
        let a: Vec<_> = (0..4)
            .map(|_| admission.try_acquire(&project, &chat_a).unwrap())
            .collect();
        assert_eq!(
            admission.try_acquire(&project, &chat_a).unwrap_err(),
            PageAdmissionError::Owner
        );
        let b: Vec<_> = (0..4)
            .map(|_| admission.try_acquire(&project, &chat_b).unwrap())
            .collect();
        assert_eq!(
            admission
                .try_acquire(&project_id("other"), &chat("chat-c"))
                .unwrap_err(),
            PageAdmissionError::Global
        );
        drop(a);
        let replacement = admission.try_acquire(&project, &chat_a).unwrap();
        drop(replacement);
        drop(b);
    }
}
