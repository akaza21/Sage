use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager, Webview};
use webview2_com::{
    IsDocumentPlayingAudioChangedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2, ICoreWebView2_3, ICoreWebView2_8, ICoreWebView2_19,
    },
    TrySuspendCompletedHandler,
};
use windows_core::{BOOL, Interface};

use super::{Event, Inputs, MemoryState, MemoryTarget, Pins, Policy, next_state};
use crate::browser::BrowserState;

#[derive(Default)]
pub(in crate::browser) struct Background {
    policy: Policy,
    timer: Option<tauri::async_runtime::JoinHandle<()>>,
    operations: Arc<AtomicUsize>,
    session: Weak<SessionPin>,
    session_watch: Option<tauri::async_runtime::JoinHandle<()>>,
}

/// The attachment contract admits one current Agent session per tab. A cached
/// capability can outlive detach, so its pin also checks the attachment lease.
pub(in crate::browser) struct SessionPin {
    app: AppHandle,
    label: String,
    lease: Option<crate::acp::browser::attachment::AttachmentLease>,
}

impl SessionPin {
    fn is_live(&self) -> bool {
        self.lease
            .as_ref()
            .is_none_or(|lease| lease.is_live() && !lease.user_controls())
    }
}

impl Drop for SessionPin {
    fn drop(&mut self) {
        if let Some(webview) = self
            .app
            .state::<BrowserState>()
            .webview_for_tab(&self.label)
        {
            refresh(&webview);
        }
    }
}

pub(in crate::browser) fn pin_session(
    webview: &Webview,
    lease: Option<crate::acp::browser::attachment::AttachmentLease>,
) -> Result<Arc<SessionPin>, String> {
    let pin = Arc::new(SessionPin {
        app: webview.app_handle().clone(),
        label: webview.label().to_owned(),
        lease,
    });
    let state = webview.app_handle().state::<BrowserState>();
    let mut tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
    let tab = tabs
        .entries
        .get_mut(webview.label())
        .ok_or("browser tab unavailable")?;
    if let Some(watcher) = tab.background.session_watch.take() {
        watcher.abort();
    }
    tab.background.session = Arc::downgrade(&pin);
    if let Some(lease) = &pin.lease {
        let mut changes = lease.control_changes();
        let weak = Arc::downgrade(&pin);
        tab.background.session_watch = Some(tauri::async_runtime::spawn(async move {
            while changes.changed().await.is_ok() {
                let Some(pin) = weak.upgrade() else {
                    break;
                };
                if let Some(webview) = pin.app.state::<BrowserState>().webview_for_tab(&pin.label) {
                    // A coalesced notification can hide a brief pin. Start a
                    // fresh grace even when the latest flags match the old ones.
                    dispatch(&webview, Event::PinsChanged);
                }
                if pin.lease.as_ref().is_some_and(|lease| !lease.is_live()) {
                    break;
                }
            }
        }));
    }
    drop(tabs);
    refresh(webview);
    Ok(pin)
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        if let Some(watcher) = self.session_watch.take() {
            watcher.abort();
        }
    }
}

/// Holds the page awake for the whole operation, including asynchronous CDP
/// completion, independently of the tab registry's close transaction.
pub(in crate::browser) struct Awake {
    app: AppHandle,
    label: String,
    operations: Arc<AtomicUsize>,
}

impl Drop for Awake {
    fn drop(&mut self) {
        // The registry temporarily removes a tab while closing it. Keep the
        // lease count correct even if that close later rolls back.
        self.operations.fetch_sub(1, Ordering::AcqRel);
        let state = self.app.state::<BrowserState>();
        if let Some(webview) = state.webview_for_tab(&self.label) {
            refresh(&webview);
        }
    }
}

/// The only operation wake path. with_webview executes inline on the owner
/// thread, so PageVisibility can resume before show without another frame.
pub(in crate::browser) fn ensure_awake(webview: &Webview) -> Result<Awake, String> {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let operations = {
        let state = app.state::<BrowserState>();
        let mut tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        let tab = tabs
            .entries
            .get_mut(&label)
            .ok_or("browser tab unavailable")?;
        let operations = tab.background.operations.clone();
        operations
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_add(1)
            })
            .map_err(|_| "browser operation capacity")?;
        operations
    };
    let awake = Awake {
        app: app.clone(),
        label: label.clone(),
        operations,
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core =
                    unsafe { platform.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
                // Wake also covers a native suspend whose completion has not
                // reached the policy yet. The transition owns the one Resume.
                update(&app, &label, &core, Event::Wake)
            })();
            let _ = sender.send(result);
        })
        .map_err(|e| e.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| e.to_string())??;
    Ok(awake)
}

fn resume(core: &ICoreWebView2, label: &str, target: MemoryTarget) -> Result<(), String> {
    if let Ok(core) = core.cast::<ICoreWebView2_3>() {
        unsafe { core.Resume() }.map_err(|e| e.to_string())?;
    }
    // A suspended view ignores target writes and Resume resets it to NORMAL.
    // Reconcile afterward so a hidden operation wake remains LOW.
    set_memory_target(core, label, target);
    Ok(())
}

fn set_memory_target(core: &ICoreWebView2, label: &str, target: MemoryTarget) {
    if let Ok(memory) = core.cast::<ICoreWebView2_19>() {
        let target = match target {
            MemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
            MemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        let mut current = target;
        if unsafe { memory.MemoryUsageTargetLevel(&mut current) }.is_ok() && current == target {
            return;
        }
        if let Err(error) = unsafe { memory.SetMemoryUsageTargetLevel(target) } {
            tracing::warn!(%label, %error, "browser memory target unavailable");
        }
    }
}

pub(in crate::browser) fn refresh(webview: &Webview) {
    dispatch(webview, Event::Changed);
}

fn dispatch(webview: &Webview, event: Event) {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let _ = webview.with_webview(move |platform| {
        let result = unsafe { platform.controller().CoreWebView2() }
            .map_err(|e| e.to_string())
            .and_then(|core| update(&app, &label, &core, event));
        if let Err(error) = result {
            tracing::warn!(%label, %error, "browser background update failed");
        }
    });
}

fn update(app: &AppHandle, label: &str, core: &ICoreWebView2, event: Event) -> Result<(), String> {
    let state = app.state::<BrowserState>();
    let Some(webview) = state.webview_for_tab(label) else {
        return Ok(());
    };
    let minimized = webview.window().is_minimized().unwrap_or(true);
    let mut audio = BOOL(0);
    let audio = core
        .cast::<ICoreWebView2_8>()
        .and_then(|core| unsafe { core.IsDocumentPlayingAudio(&mut audio) })
        .map(|()| audio.as_bool())
        .unwrap_or(true);
    let download = state
        .downloads
        .lock()
        .map(|downloads| downloads.has_active(label))
        .unwrap_or(true);
    let annotate = state.annotate.is_annotating(label);
    // Keep any upgraded session alive outside `tabs`; its final Drop refreshes
    // this tab and must never run while the registry is locked.
    let session = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .entries
        .get(label)
        .and_then(|tab| tab.background.session.upgrade());
    let session_live = session.as_ref().is_some_and(|pin| pin.is_live());
    let transition = {
        let mut tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        let Some(tab) = tabs.entries.get_mut(label) else {
            return Ok(());
        };
        let pins = Pins {
            audio,
            download,
            annotate,
            agent: tab.background.operations.load(Ordering::Acquire) != 0 || session_live,
            loading: tab.loading,
            // TODO(M2): provide a supported tab-scoped DevTools-open signal.
            devtools: false,
        };
        let Some(transition) = next_state(
            Inputs {
                current: tab.background.policy,
                placed: tab.visibility.is_placed(),
                minimized,
                pins,
                event,
            },
            Instant::now(),
        ) else {
            // A completed suspend can arrive after wake or placement. Never
            // leave its successful native side effect behind.
            let stale_resume = matches!(event, Event::Completed(_, true))
                && tab.background.policy.state != MemoryState::Suspended;
            let target = MemoryTarget::for_placement(tab.visibility.is_placed(), minimized);
            drop(tabs);
            if stale_resume {
                resume(core, label, target)?;
            }
            return Ok(());
        };
        if let Some(timer) = tab.background.timer.take() {
            timer.abort();
        }
        tab.background.policy = transition.next;
        transition
    };
    if transition.resume {
        resume(core, label, transition.target)?;
    } else {
        set_memory_target(core, label, transition.target);
    }
    let generation = transition.next.generation;
    if transition.suspend {
        let suspended_app = app.clone();
        let suspended_label = label.to_owned();
        let suspended_core = core.clone();
        let completed = TrySuspendCompletedHandler::create(Box::new(move |result, successful| {
            let _ = update(
                &suspended_app,
                &suspended_label,
                &suspended_core,
                Event::Completed(generation, result.is_ok() && successful),
            );
            Ok(())
        }));
        // Other runtime-owned activity may decline suspension.
        // That is a completed attempt, never a polling loop.
        if core
            .cast::<ICoreWebView2_3>()
            .and_then(|suspend| unsafe { suspend.TrySuspend(&completed) })
            .is_err()
        {
            update(app, label, core, Event::Completed(generation, false))?;
        }
    }
    if let Some(delay) = transition.sleep {
        let timer_app = app.clone();
        let timer_label = label.to_owned();
        let timer = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(webview) = timer_app
                .state::<BrowserState>()
                .webview_for_tab(&timer_label)
            {
                dispatch(&webview, Event::Timer(generation));
            }
        });
        let mut tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        if let Some(tab) = tabs.entries.get_mut(label)
            && tab.background.policy.generation == generation
        {
            tab.background.timer = Some(timer);
        } else {
            timer.abort();
        }
    }
    Ok(())
}

pub(in crate::browser) async fn register(webview: &Webview) -> Result<(), String> {
    let app = webview.app_handle().clone();
    let label = webview.label().to_owned();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core =
                    unsafe { platform.controller().CoreWebView2() }.map_err(|e| e.to_string())?;
                if let Ok(audio) = core.cast::<ICoreWebView2_8>() {
                    let changed_app = app.clone();
                    let changed_label = label.clone();
                    let handler =
                        IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |_, _| {
                            if let Some(webview) = changed_app
                                .state::<BrowserState>()
                                .webview_for_tab(&changed_label)
                            {
                                dispatch(&webview, Event::Wake);
                            }
                            Ok(())
                        }));
                    let mut token = 0;
                    unsafe { audio.add_IsDocumentPlayingAudioChanged(&handler, &mut token) }
                        .map_err(|e| e.to_string())?;
                }
                update(&app, &label, &core, Event::Changed)
            })();
            let _ = sender.send(result);
        })
        .map_err(|e| e.to_string())?;
    receiver.await.map_err(|e| e.to_string())?
}
