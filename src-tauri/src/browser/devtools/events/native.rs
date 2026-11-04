//! Scoped native diagnostic setup and teardown. Every domain enable is issued
//! in the same UI callback as registration, under its authority and wake epoch.
use super::*;
use std::{sync::mpsc, time::Duration};
use tauri::Webview;
use webview2_com::{CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR, DevToolsProtocolEventReceivedEventHandler};
use windows_core::{PCWSTR, PWSTR};
use crate::acp::browser::{BrowserFailure, permission::BrowserDispatchPermit};
use crate::browser::devtools::cdp;

pub(in crate::browser::devtools) fn attach(
    webview: &Webview, owner: Option<DiagnosticScope>, permit: Option<BrowserDispatchPermit>,
) -> Result<Arc<Mutex<DiagnosticRings>>, BrowserFailure> {
    let _attachment = NATIVE_ATTACHMENT_LOCK.lock().map_err(|_| BrowserFailure::Unavailable)?;
    let label = webview.label().to_owned();
    let cancelled = Arc::new(AtomicBool::new(false));
    let callback_cancelled = cancelled.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let (enabled_sender, enabled_receiver) = mpsc::sync_channel(3);
    let cleanup_webview = webview.clone();
    let wait_permit = permit.clone();
    webview.with_webview(move |platform| {
        let result = (|| {
            let core = unsafe { platform.controller().CoreWebView2() }.map_err(|_| BrowserFailure::Unavailable)?;
            let install = || {
                let lease = NATIVE_SUBSCRIPTIONS.with(|table| {
                    table.borrow_mut().attach_scoped_once(&label, owner.clone(), &callback_cancelled, |event, rings| {
                        let wide = event.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                        let source = unsafe { core.GetDevToolsProtocolEventReceiver(PCWSTR(wide.as_ptr())) }.map_err(|_| BrowserFailure::Unavailable)?;
                        let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_core, args| {
                            if let Some(args) = args {
                                let mut raw = PWSTR::null();
                                unsafe { args.ParameterObjectAsJson(&mut raw)? };
                                ingest(&rings, event, &CoTaskMemPWSTR::from(raw).to_string());
                            }
                            Ok(())
                        }));
                        let mut token = 0;
                        unsafe { source.add_DevToolsProtocolEventReceived(&handler, &mut token) }.map_err(|_| BrowserFailure::Unavailable)?;
                        Ok(NativeSubscription { source, token })
                    })
                })?;
                if !lease.existing {
                    for method in ["Runtime.enable", "Log.enable", "Network.enable"] {
                        let enabled_sender = enabled_sender.clone();
                        let completed = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, _| {
                            let _ = enabled_sender.try_send(result.map_err(|_| BrowserFailure::Unavailable));
                            Ok(())
                        }));
                        let method = method.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
                        let params = [b'{' as u16, b'}' as u16, 0];
                        if unsafe { core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &completed) }.is_err() {
                            teardown(&label, &lease.rings, &core);
                            return Err(BrowserFailure::Unavailable);
                        }
                    }
                }
                Ok(lease)
            };
            let authorized = || match &permit { Some(permit) => permit.with_live_lock(install), None => install() };
            match &owner {
                Some(owner) => owner.with_registered_diagnostics(|| {
                    let lease = authorized()?;
                    let release: Option<Box<dyn FnOnce() + Send>> = if lease.existing { None } else {
                        let rings = lease.rings.clone();
                        Some(Box::new(move || suspend(&cleanup_webview, rings)))
                    };
                    Ok((lease, release))
                }),
                None => authorized(),
            }
        })();
        if let Err(unsent) = sender.send(result) && let Ok(lease) = unsent.0 && !lease.existing {
            if let Ok(core) = unsafe { platform.controller().CoreWebView2() } { teardown(&label, &lease.rings, &core); }
        }
    }).map_err(|_| BrowserFailure::Unavailable)?;
    let lease = receive(&receiver, wait_permit.as_ref()).inspect_err(|_| { cancelled.store(true, Ordering::Release); })?;
    if lease.existing { return Ok(lease.rings); }
    let enabled = (0..3).try_for_each(|_| {
        // Sleep releases diagnostics but preserves the requested operation.
        if lease.rings.lock().is_ok_and(|rings| rings.is_suspended()) && wait_permit.as_ref().is_some_and(|permit| permit.is_live()) { return Ok(()); }
        receive(&enabled_receiver, wait_permit.as_ref())
    });
    if let Err(error) = enabled {
        suspend(webview, lease.rings);
        return Err(error);
    }
    if lease.rings.lock().is_ok_and(|rings| rings.is_suspended()) && wait_permit.as_ref().is_some_and(|permit| permit.is_live()) { return Ok(lease.rings); }
    if lease.activate().is_err() { suspend(webview, lease.rings); return Err(BrowserFailure::Unavailable); }
    Ok(lease.rings)
}

/// Setup waits are interruptible too: revoking authority must not leave a
/// worker waiting for domain-enable callbacks until their timeout.
fn receive<T>(receiver: &mpsc::Receiver<Result<T, BrowserFailure>>, permit: Option<&BrowserDispatchPermit>) -> Result<T, BrowserFailure> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if permit.is_some_and(|permit| !permit.is_live()) { return Err(BrowserFailure::Denied); }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() { return Err(BrowserFailure::Unavailable); }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {},
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(BrowserFailure::Unavailable),
        }
    }
}

type Core = webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

fn teardown(label: &str, rings: &Arc<Mutex<DiagnosticRings>>, core: &Core) {
    let detached = NATIVE_SUBSCRIPTIONS.with(|table| {
        let mut table = table.borrow_mut();
        if table.tabs.get(label).is_some_and(|tab| Arc::ptr_eq(&tab.rings, rings)) { table.detach(label); true } else { false }
    });
    if detached {
        cdp::release_developer_objects(core);
        for method in ["Runtime.disable", "Log.disable", "Network.disable"] { cdp::notify(core, method, "{}"); }
    }
}

pub(in crate::browser::devtools) fn suspend(webview: &Webview, rings: Arc<Mutex<DiagnosticRings>>) {
    if let Ok(mut rings) = rings.lock() { rings.suspend(); }
    let label = webview.label().to_owned();
    let _ = webview.with_webview(move |platform| {
        if let Ok(core) = unsafe { platform.controller().CoreWebView2() } { teardown(&label, &rings, &core); }
    });
}

pub(crate) fn detach(webview: &Webview) -> Result<(), BrowserFailure> {
    let _attachment = NATIVE_ATTACHMENT_LOCK.lock().map_err(|_| BrowserFailure::Unavailable)?;
    let label = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    webview.with_webview(move |_| {
        NATIVE_SUBSCRIPTIONS.with(|table| table.borrow_mut().detach(&label));
        let _ = sender.send(());
    }).map_err(|_| BrowserFailure::Unavailable)?;
    receiver.recv_timeout(Duration::from_secs(5)).map_err(|_| BrowserFailure::Unavailable)
}

struct NativeSubscription {
    source: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2DevToolsProtocolEventReceiver,
    token: i64,
}

impl Subscription for NativeSubscription {
    fn unregister(&self) { let _ = unsafe { self.source.remove_DevToolsProtocolEventReceived(self.token) }; }
}

thread_local! {
    static NATIVE_SUBSCRIPTIONS: std::cell::RefCell<SubscriptionTable<NativeSubscription>> = std::cell::RefCell::new(SubscriptionTable::default());
}
static NATIVE_ATTACHMENT_LOCK: Mutex<()> = Mutex::new(());
