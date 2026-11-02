use std::{cell::RefCell, sync::Arc};
use tauri::Webview;
use tracing::warn;
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::{
            EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART,
            GetWindowThreadProcessId, IsChild, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
        },
    },
};

use super::native::{Change, PendingViewport, request};

struct Target {
    hooks: Vec<HWINEVENTHOOK>,
    host: HWND,
    window: HWND,
    processes: [u32; 2],
    webview: Webview,
    pending: Arc<PendingViewport>,
}

thread_local! {
    // OUTOFCONTEXT callbacks run on the subscribing GUI thread. No HWND or
    // callback context crosses threads; only Tauri handles enter queued work.
    static TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
}

pub(super) fn observe(webview: Webview) {
    if webview.label() != "main" {
        return;
    }
    let target = webview.clone();
    if let Err(error) = webview.with_webview(move |platform| {
        let mut host = HWND::default();
        let Ok(window) = target.window().hwnd() else {
            return;
        };
        if let Err(error) = unsafe { platform.controller().ParentWindow(&mut host) } {
            warn!(?error, "main viewport host is unavailable");
            return;
        }
        let mut processes = [0; 2];
        unsafe {
            GetWindowThreadProcessId(host, Some(&mut processes[0]));
        }
        let browser = unsafe {
            platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.BrowserProcessId(&mut processes[1]))
        };
        if processes.contains(&0) || browser.is_err() {
            warn!("main viewport process identities are unavailable");
            return;
        }
        let existing = TARGET.with(|slot| {
            slot.borrow()
                .as_ref()
                .filter(|target| {
                    target.host == host && target.window == window && target.processes == processes
                })
                .map(|target| (target.webview.clone(), target.pending.clone()))
        });
        if let Some((webview, pending)) = existing {
            request(webview, pending, Change::NativeWindow);
            return;
        }
        stop();
        // WebView2 owns descendants in its windowed host. Filter by that exact
        // SDK-provided subtree and its host/browser processes, without relying
        // on Chromium's private classes or monitoring unrelated applications.
        let mut hooks = Vec::with_capacity(processes.len());
        for (index, process) in processes.into_iter().enumerate() {
            if processes[..index].contains(&process) {
                continue;
            }
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(location_changed),
                    process,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                )
            };
            if hook.is_invalid() {
                for hook in hooks {
                    let _ = unsafe { UnhookWinEvent(hook) };
                }
                warn!("main viewport geometry subscription failed");
                return;
            }
            hooks.push(hook);
        }
        let mut window_process = 0;
        unsafe {
            GetWindowThreadProcessId(window, Some(&mut window_process));
        }
        if window_process == 0 {
            for hook in hooks {
                let _ = unsafe { UnhookWinEvent(hook) };
            }
            warn!("main viewport window process is unavailable");
            return;
        }
        let minimize = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_MINIMIZESTART,
                EVENT_SYSTEM_MINIMIZEEND,
                None,
                Some(location_changed),
                window_process,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if minimize.is_invalid() {
            for hook in hooks {
                let _ = unsafe { UnhookWinEvent(hook) };
            }
            warn!("main viewport restore subscription failed");
            return;
        }
        hooks.push(minimize);
        let pending = Arc::new(PendingViewport::default());
        TARGET.with(|slot| {
            *slot.borrow_mut() = Some(Target {
                hooks,
                host,
                window,
                processes,
                webview: target.clone(),
                pending: pending.clone(),
            });
        });
        request(target, pending, Change::NativeWindow);
    }) {
        warn!(?error, "main viewport subscription dispatch failed");
    }
}

pub(super) fn reconcile() {
    let target = TARGET.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|target| (target.webview.clone(), target.pending.clone()))
    });
    if let Some((webview, pending)) = target {
        request(webview, pending, Change::NativeWindow);
    }
}

pub(super) fn stop() {
    let target = TARGET.with(|slot| slot.borrow_mut().take());
    if let Some(target) = target {
        target.pending.close();
        // Unsubscribe on the same GUI thread that installed the callback.
        for hook in target.hooks {
            let _ = unsafe { UnhookWinEvent(hook) };
        }
    }
}

unsafe extern "system" fn location_changed(
    hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if event == EVENT_SYSTEM_MINIMIZESTART || event == EVENT_SYSTEM_MINIMIZEEND {
        let matches = TARGET.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|target| target.hooks.contains(&hook) && target.window == hwnd)
        });
        if matches {
            reconcile();
        }
        return;
    }
    if object != OBJID_WINDOW.0 || child != 0 {
        return;
    }
    let host = TARGET.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|target| target.hooks.contains(&hook))
            .map(|target| target.host)
    });
    let Some(host) = host else {
        return;
    };
    if !unsafe { IsChild(host, hwnd) }.as_bool() {
        return;
    }
    // Notifications contain no historical rectangle. Even an identical current
    // rectangle may hide restore -> capture rollback before callback delivery.
    // The coalesced worker samples the live renderer instead of caching sizes.
    let target = TARGET.with(|slot| {
        slot.borrow()
            .as_ref()
            .filter(|target| target.hooks.contains(&hook))
            .map(|target| (target.webview.clone(), target.pending.clone()))
    });
    if let Some((webview, pending)) = target {
        request(webview, pending, Change::RendererWindow);
    }
}
