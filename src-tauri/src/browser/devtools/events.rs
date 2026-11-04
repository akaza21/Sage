use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use serde_json::Value;

use super::diagnostics::DiagnosticRings;

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
    fn attach_once(
        &mut self,
        label: &str,
        cancelled: &AtomicBool,
        mut register: impl FnMut(
            &'static str,
            Arc<Mutex<DiagnosticRings>>,
        ) -> Result<R, crate::acp::browser::BrowserFailure>,
    ) -> Result<AttachmentLease, crate::acp::browser::BrowserFailure> {
        use crate::acp::browser::BrowserFailure;
        if let Some(tab) = self.tabs.get(label) {
            match tab.state.load(Ordering::Acquire) {
                ACTIVE => {
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
pub(super) fn attach(
    webview: &tauri::Webview,
) -> Result<Arc<Mutex<DiagnosticRings>>, crate::acp::browser::BrowserFailure> {
    use std::{sync::mpsc, time::Duration};

    use webview2_com::{CoTaskMemPWSTR, DevToolsProtocolEventReceivedEventHandler};
    use windows_core::{PCWSTR, PWSTR};

    use crate::acp::browser::BrowserFailure;

    let _attachment = NATIVE_ATTACHMENT_LOCK
        .lock()
        .map_err(|_| BrowserFailure::Unavailable)?;
    let label = webview.label().to_owned();
    let cancelled = Arc::new(AtomicBool::new(false));
    let callback_cancelled = cancelled.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|_| BrowserFailure::Unavailable)?;
                NATIVE_SUBSCRIPTIONS.with(|table| {
                    let mut table = table.borrow_mut();
                    table.attach_once(&label, &callback_cancelled, |event, rings| {
                        let wide = event.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                        let source =
                            unsafe { core.GetDevToolsProtocolEventReceiver(PCWSTR(wide.as_ptr())) }
                                .map_err(|_| BrowserFailure::Unavailable)?;
                        let event_rings = rings.clone();
                        let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(
                            move |_core, args| {
                                if let Some(args) = args {
                                    let mut raw = PWSTR::null();
                                    unsafe { args.ParameterObjectAsJson(&mut raw)? };
                                    let payload = CoTaskMemPWSTR::from(raw).to_string();
                                    ingest(&event_rings, event, &payload);
                                }
                                Ok(())
                            },
                        ));
                        let mut token = 0;
                        unsafe { source.add_DevToolsProtocolEventReceived(&handler, &mut token) }
                            .map_err(|_| BrowserFailure::Unavailable)?;
                        Ok(NativeSubscription { source, token })
                    })
                })
            })();
            if let Err(unsent) = sender.send(result)
                && let Ok(lease) = unsent.0
                && !lease.existing
            {
                NATIVE_SUBSCRIPTIONS
                    .with(|table| table.borrow_mut().detach_if(&label, &lease.state));
            }
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    let lease = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(result) => result?,
        Err(_) => {
            cancelled.store(true, Ordering::Release);
            return Err(BrowserFailure::Unavailable);
        }
    };
    if lease.existing {
        return Ok(lease.rings);
    }
    for domain in ["Runtime.enable", "Log.enable", "Network.enable"] {
        if let Err(error) = super::cdp::call(webview, domain, "{}", 4096, Duration::from_secs(5)) {
            lease.state.store(FAILED, Ordering::Release);
            let _ = detach_failed(webview, lease.state);
            return Err(error);
        }
    }
    lease.state.store(ACTIVE, Ordering::Release);
    Ok(lease.rings)
}

#[cfg(windows)]
struct NativeSubscription {
    source:
        webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DevToolsProtocolEventReceiver,
    token: i64,
}

#[cfg(windows)]
impl Subscription for NativeSubscription {
    fn unregister(&self) {
        let _ = unsafe { self.source.remove_DevToolsProtocolEventReceived(self.token) };
    }
}

#[cfg(windows)]
thread_local! {
    static NATIVE_SUBSCRIPTIONS: std::cell::RefCell<SubscriptionTable<NativeSubscription>> =
        std::cell::RefCell::new(SubscriptionTable::default());
}

#[cfg(windows)]
static NATIVE_ATTACHMENT_LOCK: Mutex<()> = Mutex::new(());

#[cfg(windows)]
pub(crate) fn detach(webview: &tauri::Webview) -> Result<(), crate::acp::browser::BrowserFailure> {
    let _attachment = NATIVE_ATTACHMENT_LOCK
        .lock()
        .map_err(|_| crate::acp::browser::BrowserFailure::Unavailable)?;
    detach_inner(webview)
}

#[cfg(windows)]
fn detach_inner(webview: &tauri::Webview) -> Result<(), crate::acp::browser::BrowserFailure> {
    use crate::acp::browser::BrowserFailure;
    use std::{sync::mpsc, time::Duration};

    let label = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |_| {
            NATIVE_SUBSCRIPTIONS.with(|table| table.borrow_mut().detach(&label));
            let _ = sender.send(());
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| BrowserFailure::Unavailable)
}

#[cfg(windows)]
fn detach_failed(
    webview: &tauri::Webview,
    state: Arc<AtomicU8>,
) -> Result<(), crate::acp::browser::BrowserFailure> {
    use crate::acp::browser::BrowserFailure;
    let label = webview.label().to_owned();
    webview
        .with_webview(move |_| {
            NATIVE_SUBSCRIPTIONS.with(|table| table.borrow_mut().detach_if(&label, &state));
        })
        .map_err(|_| BrowserFailure::Unavailable)
}
