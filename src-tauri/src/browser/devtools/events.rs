use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use serde_json::Value;

use super::diagnostics::DiagnosticRings;
use crate::acp::browser::attachment::DiagnosticScope;

pub(super) const EVENTS: [&str; 4] = [
    "Runtime.consoleAPICalled",
    "Log.entryAdded",
    "Network.requestWillBeSent",
    "Network.responseReceived",
];

pub(super) trait Subscription {
    fn unregister(&self);
}

struct TabSubscription<R> {
    owner: Option<DiagnosticScope>,
    rings: Arc<Mutex<DiagnosticRings>>,
    registrations: Vec<R>,
    state: Arc<AtomicU8>,
}

const PENDING: u8 = 0;
const ACTIVE: u8 = 1;
const FAILED: u8 = 2;

struct AttachmentLease {
    rings: Arc<Mutex<DiagnosticRings>>,
    state: Arc<AtomicU8>,
    existing: bool,
}

impl AttachmentLease {
    fn activate(&self) -> Result<(), crate::acp::browser::BrowserFailure> {
        if self.rings.lock().is_ok_and(|rings| rings.is_suspended()) { return Err(crate::acp::browser::BrowserFailure::Denied); }
        self.state.compare_exchange(PENDING, ACTIVE, Ordering::AcqRel, Ordering::Acquire).map(|_| ()).map_err(|_| crate::acp::browser::BrowserFailure::Denied)
    }
}

pub(super) struct SubscriptionTable<R: Subscription> {
    tabs: HashMap<String, TabSubscription<R>>,
}

impl<R: Subscription> Default for SubscriptionTable<R> {
    fn default() -> Self {
        Self {
            tabs: HashMap::new(),
        }
    }
}

impl<R: Subscription> SubscriptionTable<R> {
    #[cfg(test)]
    fn attach_once(
        &mut self,
        label: &str,
        cancelled: &AtomicBool,
        register: impl FnMut(
            &'static str,
            Arc<Mutex<DiagnosticRings>>,
        ) -> Result<R, crate::acp::browser::BrowserFailure>,
    ) -> Result<AttachmentLease, crate::acp::browser::BrowserFailure> {
        self.attach_scoped_once(label, None, cancelled, register)
    }

    fn attach_scoped_once(
        &mut self, label: &str, owner: Option<DiagnosticScope>, cancelled: &AtomicBool,
        mut register: impl FnMut(&'static str, Arc<Mutex<DiagnosticRings>>) -> Result<R, crate::acp::browser::BrowserFailure>,
    ) -> Result<AttachmentLease, crate::acp::browser::BrowserFailure> {
        use crate::acp::browser::BrowserFailure;
        if let Some(tab) = self.tabs.get(label) {
            match tab.state.load(Ordering::Acquire) {
                ACTIVE if tab.owner == owner && tab.rings.lock().is_ok_and(|rings| !rings.is_suspended()) => {
                    return Ok(AttachmentLease {
                        rings: tab.rings.clone(),
                        state: tab.state.clone(),
                        existing: true,
                    });
                }
                PENDING => return Err(BrowserFailure::Unavailable),
                _ => self.detach(label),
            }
        }
        let rings = Arc::new(Mutex::new(DiagnosticRings::default()));
        let state = Arc::new(AtomicU8::new(PENDING));
        let mut registrations: Vec<R> = Vec::new();
        for event in EVENTS {
            if cancelled.load(Ordering::Acquire) {
                for registration in registrations.iter().rev() {
                    registration.unregister();
                }
                return Err(BrowserFailure::Unavailable);
            }
            match register(event, rings.clone()) {
                Ok(registration) => registrations.push(registration),
                Err(error) => {
                    for registration in registrations.iter().rev() {
                        registration.unregister();
                    }
                    return Err(error);
                }
            }
        }
        if cancelled.load(Ordering::Acquire) {
            for registration in registrations.iter().rev() {
                registration.unregister();
            }
            return Err(BrowserFailure::Unavailable);
        }
        self.tabs.insert(
            label.to_owned(),
            TabSubscription {
                owner,
                rings: rings.clone(),
                registrations,
                state: state.clone(),
            },
        );
        Ok(AttachmentLease {
            rings,
            state,
            existing: false,
        })
    }

    #[cfg(test)]
    fn detach_if(&mut self, label: &str, state: &Arc<AtomicU8>) {
        if self
            .tabs
            .get(label)
            .is_some_and(|tab| Arc::ptr_eq(&tab.state, state))
        {
            self.detach(label);
        }
    }

    pub(super) fn detach(&mut self, label: &str) {
        if let Some(tab) = self.tabs.remove(label) {
            tab.state.store(FAILED, Ordering::Release);
            if let Ok(mut rings) = tab.rings.lock() { rings.suspend(); }
            for registration in tab.registrations.iter().rev() {
                registration.unregister();
            }
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.tabs.len()
    }
}

impl<R: Subscription> Drop for SubscriptionTable<R> {
    fn drop(&mut self) {
        for tab in self.tabs.values() {
            for registration in tab.registrations.iter().rev() {
                registration.unregister();
            }
        }
    }
}

pub(super) fn ingest(rings: &Arc<Mutex<DiagnosticRings>>, event: &str, payload: &str) {
    if payload.len() > 65_536 {
        return;
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    let Ok(mut rings) = rings.lock() else {
        return;
    };
    match event {
        "Runtime.consoleAPICalled" => {
            let level = value["type"].as_str().unwrap_or("log");
            let text = value["args"]
                .as_array()
                .into_iter()
                .flatten()
                .take(8)
                .filter_map(|argument| {
                    argument["value"]
                        .as_str()
                        .or_else(|| argument["description"].as_str())
                })
                .collect::<Vec<_>>()
                .join(" ");
            rings.push_console(level, &text);
        }
        "Log.entryAdded" => {
            let entry = &value["entry"];
            rings.push_console(
                entry["level"].as_str().unwrap_or("log"),
                entry["text"].as_str().unwrap_or(""),
            );
        }
        "Network.requestWillBeSent" => {
            let request = &value["request"];
            if let (Some(method), Some(url)) = (request["method"].as_str(), request["url"].as_str())
            {
                rings.push_network(method, url, 0);
            }
        }
        "Network.responseReceived" => {
            let response = &value["response"];
            if let (Some(url), Some(status)) =
                (response["url"].as_str(), response["status"].as_u64())
                && let Ok(status) = u16::try_from(status)
            {
                rings.push_network("RESPONSE", url, status);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
#[path = "events/subscription_tests.rs"]
mod subscription_tests;

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub(super) use native::{attach, suspend};
#[cfg(windows)]
pub(crate) use native::detach;
