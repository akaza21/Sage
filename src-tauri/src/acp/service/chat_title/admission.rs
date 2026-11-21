use crate::acp::{runtime::DriverMailbox, store::title_attempt};
use sage_acp_protocol::scheduler::RuntimeOwner;
use sage_db::database::Database;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};

/// Only live runtime registrations are retained, never pending title work.
#[derive(Default)]
pub(crate) struct TitleAdmission {
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    busy: bool,
    foreground: u64,
    active_turns: usize,
    selected: Option<RuntimeOwner>,
    runtimes: Vec<RuntimeWake>,
}
struct RuntimeWake {
    database: Database,
    owner: RuntimeOwner,
    mailbox: Weak<DriverMailbox>,
    ready: Arc<AtomicBool>,
}
pub(super) struct Wake {
    pub(super) owner: RuntimeOwner,
    pub(super) ready: Arc<AtomicBool>,
}
impl TitleAdmission {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn foreground(&self) {
        self.state().foreground += 1;
    }
    pub(super) fn turn(self: &Arc<Self>) -> ForegroundLease {
        let mut state = self.state();
        state.foreground += 1;
        state.active_turns += 1;
        ForegroundLease(Arc::clone(self))
    }
    pub(super) fn generation(&self) -> u64 {
        self.state().foreground
    }
    pub(super) fn acquire(self: &Arc<Self>, owner: &RuntimeOwner) -> Option<(Lease, u64)> {
        let mut state = self.state();
        if state.busy
            || state.active_turns != 0
            || state
                .selected
                .as_ref()
                .is_some_and(|selected| selected != owner)
        {
            return None;
        }
        state.selected = None;
        if let Some(runtime) = state.runtimes.iter().find(|r| &r.owner == owner) {
            runtime.ready.store(false, Ordering::Release);
        }
        state.busy = true;
        Some((Lease(Arc::clone(self)), state.foreground))
    }
    pub(super) fn register(
        self: &Arc<Self>,
        database: Database,
        owner: RuntimeOwner,
        mailbox: &Arc<DriverMailbox>,
    ) -> Option<Wake> {
        let mut state = self.state();
        state.runtimes.retain(|runtime| {
            runtime
                .mailbox
                .upgrade()
                .is_some_and(|mailbox| mailbox.title_wake_available())
        });
        if state.runtimes.len() >= crate::acp::service::LIVE_RUNTIME_LANES {
            return None;
        }
        mailbox.bind_title_admission(Arc::downgrade(self));
        let ready = Arc::new(AtomicBool::new(false));
        state.runtimes.push(RuntimeWake {
            database,
            owner: owner.clone(),
            mailbox: Arc::downgrade(mailbox),
            ready: Arc::clone(&ready),
        });
        Some(Wake { owner, ready })
    }
    pub(super) fn unregister(&self, owner: &RuntimeOwner) {
        {
            let mut state = self.state();
            state.runtimes.retain(|runtime| &runtime.owner != owner);
            if state.selected.as_ref() == Some(owner) {
                state.selected = None;
            }
        }
        self.wake_pending();
    }
    pub(crate) fn wake_pending(&self) {
        let (database, owners, generation) = {
            let mut state = self.state();
            if state.busy || state.active_turns != 0 {
                return;
            }
            if let Some(owner) = &state.selected {
                if let Some(runtime) = state.runtimes.iter().find(|r| &r.owner == owner)
                    && let Some(mailbox) = runtime.mailbox.upgrade()
                    && mailbox.title_wake_available()
                {
                    runtime.ready.store(true, Ordering::Release);
                    mailbox.wake_title();
                    return;
                }
                state.selected = None;
            }
            let Some(first) = state.runtimes.first() else {
                return;
            };
            (
                first.database.clone(),
                state
                    .runtimes
                    .iter()
                    .filter(|r| {
                        r.mailbox
                            .upgrade()
                            .is_some_and(|mailbox| mailbox.title_wake_available())
                    })
                    .map(|r| r.owner.clone())
                    .collect::<Vec<_>>(),
                state.foreground,
            )
        };
        // A single bounded durable lookup; no database lock nests inside the
        // admission lock, and inactive runtimes have no timer or listener.
        let selected = title_attempt::pending_owner(&database.connection(), &owners)
            .ok()
            .flatten();
        let Some(owner) = selected else { return };
        let mut state = self.state();
        if state.busy
            || state.active_turns != 0
            || state.selected.is_some()
            || state.foreground != generation
        {
            return;
        }
        if let Some(runtime) = state.runtimes.iter().find(|r| r.owner == owner)
            && let Some(mailbox) = runtime.mailbox.upgrade()
        {
            runtime.ready.store(true, Ordering::Release);
            if mailbox.wake_title() {
                state.selected = Some(owner);
            } else {
                runtime.ready.store(false, Ordering::Release);
            }
        }
    }
}
pub(crate) struct ForegroundLease(Arc<TitleAdmission>);
impl Drop for ForegroundLease {
    fn drop(&mut self) {
        self.0.state().active_turns -= 1;
        self.0.wake_pending();
    }
}
pub(super) struct Lease(Arc<TitleAdmission>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.state().busy = false;
        self.0.wake_pending();
    }
}
