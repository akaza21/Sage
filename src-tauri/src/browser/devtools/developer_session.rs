//! A single privileged CDP session per physical tab. Closing/opening sessions
//! occupy the slot until native completion, bounding even stalled callbacks.
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tauri::{Manager, Webview};

use crate::acp::browser::{
    BrowserFailure, attachment::DeveloperScope, permission::BrowserDispatchPermit,
};
use crate::browser::BrowserState;

mod completion;
mod native;
mod slot;
pub(super) use completion::Completion;
use completion::CompletionTarget;
pub(in crate::browser) use slot::Slot;
use slot::{SharedSlot, TabSession};

pub(in crate::browser) struct Session {
    webview: Webview,
    slot: Weak<SharedSlot>,
    state: Mutex<State>,
    pub(super) serializer: super::cdp_port::evaluate::Serializer,
}

#[derive(Default)]
struct State {
    phase: Phase,
    busy: bool,
    sleep: bool,
    pending_completion: bool,
}

#[derive(Default)]
enum Phase {
    #[default]
    Opening,
    Ready(String),
    Closing,
    Detached,
    Closed,
}

impl State {
    fn reserve(&mut self) -> Result<(), BrowserFailure> {
        if !matches!(self.phase, Phase::Ready(_))
            || self.busy
            || self.pending_completion
            || self.sleep
        {
            return Err(BrowserFailure::Unavailable);
        }
        self.busy = true;
        Ok(())
    }

    fn close(&mut self, force: bool) -> Option<String> {
        self.sleep = true;
        if !force && self.busy {
            return None;
        }
        let id = match &self.phase {
            Phase::Opening => None,
            Phase::Ready(id) => Some(id.clone()),
            Phase::Closing | Phase::Detached | Phase::Closed => return None,
        };
        self.phase = Phase::Closing;
        id
    }

    fn attached(&mut self, id: String) -> Option<String> {
        if matches!(self.phase, Phase::Opening) {
            self.phase = Phase::Ready(id);
            None
        } else {
            Some(id)
        }
    }

    fn begin_completion(&mut self) -> Result<(), BrowserFailure> {
        if !matches!(self.phase, Phase::Ready(_)) || self.pending_completion {
            return Err(BrowserFailure::Unavailable);
        }
        self.pending_completion = true;
        Ok(())
    }

    fn detached(&mut self) -> bool {
        self.phase = if self.pending_completion {
            Phase::Detached
        } else {
            Phase::Closed
        };
        !self.pending_completion
    }

    fn completed(&mut self) -> bool {
        self.pending_completion = false;
        if matches!(self.phase, Phase::Detached) {
            self.phase = Phase::Closed;
            true
        } else {
            false
        }
    }
}

pub(super) struct Work(pub(super) Arc<Session>);

impl Drop for Work {
    fn drop(&mut self) {
        let sleep = self
            .0
            .state
            .lock()
            .map(|mut state| {
                state.busy = false;
                state.sleep
            })
            .unwrap_or(true);
        if sleep {
            self.0.close(true);
        }
    }
}

impl Session {
    pub(super) fn acquire(
        webview: &Webview,
        grant: DeveloperScope,
        permit: &BrowserDispatchPermit,
    ) -> Result<Work, BrowserFailure> {
        let slot = webview
            .app_handle()
            .state::<BrowserState>()
            .tabs
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .entries
            .get(webview.label())
            .ok_or(BrowserFailure::Unavailable)?
            .developer
            .clone();
        let (session, opening) = slot.acquire(webview.label(), &grant, |slot| Self {
            webview: webview.clone(),
            slot,
            state: Mutex::new(State {
                busy: true,
                ..State::default()
            }),
            serializer: Default::default(),
        })?;
        let work = Work(session.clone());
        if opening {
            session.start(permit.clone());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if !permit.is_live() || Instant::now() >= deadline {
                session.close(true);
                return Err(if permit.is_live() {
                    BrowserFailure::Unavailable
                } else {
                    permit.revoked_failure()
                });
            }
            let state = session
                .state
                .lock()
                .map_err(|_| BrowserFailure::Unavailable)?;
            match state.phase {
                Phase::Closing | Phase::Detached | Phase::Closed => {
                    return Err(BrowserFailure::Unavailable);
                }
                Phase::Ready(_) => return Ok(work),
                Phase::Opening => {}
            }
            drop(state);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub(super) fn with_id<T>(
        &self,
        issue: impl FnOnce(&str) -> Result<T, BrowserFailure>,
    ) -> Result<T, BrowserFailure> {
        let state = self.state.lock().map_err(|_| BrowserFailure::Unavailable)?;
        match &state.phase {
            Phase::Ready(id) if !id.is_empty() => issue(id),
            _ => Err(BrowserFailure::Denied),
        }
    }

    pub(super) fn begin_completion(self: &Arc<Self>) -> Result<Completion, BrowserFailure> {
        self.state
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .begin_completion()?;
        Ok(Completion::new(self))
    }

    pub(super) fn close(self: &Arc<Self>, force: bool) {
        let id = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.close(force)
        };
        // An unresolved attach still owns the slot. Its completion detaches
        // the returned ID; another opening cannot accumulate behind it.
        if let Some(id) = id {
            self.detach(id);
        }
    }

    fn attached(self: &Arc<Self>, id: String) {
        let detach = {
            let Ok(mut state) = self.state.lock() else {
                return;
            };
            state.attached(id)
        };
        if let Some(id) = detach {
            self.detach(id);
        }
    }

    fn finished(self: &Arc<Self>) {
        self.serializer.clear();
        if let Ok(mut state) = self.state.lock() {
            state.phase = Phase::Closed;
        }
        if let Some(slot) = self.slot.upgrade()
            && let Ok(mut current) = slot.lock()
            && current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, self))
        {
            current.take();
        }
    }

    fn detached(self: &Arc<Self>) {
        let clear = self.state.lock().is_ok_and(|mut state| state.detached());
        if clear {
            self.finished();
        }
    }
}

impl TabSession for Session {
    fn reserve(&self) -> Result<(), BrowserFailure> {
        self.state
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .reserve()
    }

    fn close(self: &Arc<Self>, force: bool) {
        Session::close(self, force);
    }
}

impl CompletionTarget for Session {
    fn complete(self: &Arc<Self>) {
        let clear = self.state.lock().is_ok_and(|mut state| state.completed());
        if clear {
            self.finished();
        }
    }
}

#[cfg(test)]
mod handoff_tests;
#[cfg(test)]
mod tests;
