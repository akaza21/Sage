//! Only the current WebView's target is attached. Never enumerate targets or
//! fall back to the default CDP session when scoped transport is unavailable.
use super::*;
use serde_json::{Value, json};
use webview2_com::{CallDevToolsProtocolMethodCompletedHandler, Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2_11}};
use windows_core::{Interface, PCWSTR};

impl Session {
    pub(super) fn start(self: &Arc<Self>, permit: BrowserDispatchPermit) {
        let session = self.clone();
        let queued = self.webview.with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }.map_err(|_| BrowserFailure::Unavailable)?;
                core.cast::<ICoreWebView2_11>().map_err(|_| BrowserFailure::Unavailable)?;
                let next_core = core.clone();
                let next = session.clone();
                let next_permit = permit.clone();
                let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |outcome, data| {
                    let target = outcome.ok().filter(|_| data.len() <= 65_536).and_then(|_| serde_json::from_str::<Value>(&data).ok())
                        .and_then(|value| value["targetInfo"]["targetId"].as_str().filter(|s| !s.is_empty() && s.len() <= 256).map(str::to_owned));
                    if let Some(target) = target {
                        next.attach(&next_core, &target, next_permit);
                    } else { next.finished(); }
                    Ok(())
                }));
                permit.with_live_lock(|| {
                    let state = session.state.lock().map_err(|_| BrowserFailure::Unavailable)?;
                    if !matches!(state.phase, Phase::Opening) { return Err(BrowserFailure::Denied); }
                    send(&core, "Target.getTargetInfo", "{}", &done)
                })
            })();
            if result.is_err() { session.finished(); }
        });
        if queued.is_err() { self.finished(); }
    }

    fn attach(self: &Arc<Self>, core: &ICoreWebView2, target: &str, permit: BrowserDispatchPermit) {
        let session = self.clone();
        let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |outcome, data| {
            let failed = outcome.is_err();
            let id = outcome.ok().filter(|_| data.len() <= 1024).and_then(|_| serde_json::from_str::<Value>(&data).ok())
                .and_then(|value| value["sessionId"].as_str().filter(|id| !id.is_empty() && id.len() <= 256).map(str::to_owned));
            if let Some(id) = id { session.attached(id); }
            else if failed { session.finished(); }
            else { session.close(true); }
            Ok(())
        }));
        let result = permit.with_live_lock(|| {
            let state = self.state.lock().map_err(|_| BrowserFailure::Unavailable)?;
            if !matches!(state.phase, Phase::Opening) { return Err(BrowserFailure::Denied); }
            send(core, "Target.attachToTarget", &json!({"targetId":target,"flatten":true}).to_string(), &done)
        });
        if result.is_err() { self.finished(); }
    }

    pub(super) fn detach(self: &Arc<Self>, id: String) {
        let session = self.clone();
        // Failure keeps this single slot quarantined until tab teardown.
        // A later grant cannot accumulate native sessions behind it.
        let _ = self.webview.with_webview(move |platform| {
            let Ok(core) = (unsafe { platform.controller().CoreWebView2() }) else { return; };
            let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |outcome, _| {
                if outcome.is_ok() { session.finished(); }
                Ok(())
            }));
            let _ = send(&core, "Target.detachFromTarget", &json!({"sessionId":id}).to_string(), &done);
        });
    }
}

fn send(core: &ICoreWebView2, method: &str, params: &str, done: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CallDevToolsProtocolMethodCompletedHandler) -> Result<(), BrowserFailure> {
    let method = super::super::cdp::wide(method);
    let params = super::super::cdp::wide(params);
    unsafe { core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), done) }.map_err(|_| BrowserFailure::Unavailable)
}
