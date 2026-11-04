use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::Webview;
use tracing::debug;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetClientRect, IsIconic},
};

pub(super) enum Change {
    NativeWindow,
    RendererWindow,
}

#[derive(Default)]
pub(super) struct PendingViewport {
    closed: AtomicBool,
    queued: AtomicBool,
    dirty: AtomicBool,
    bounds_dirty: AtomicBool,
    renderer_dirty: AtomicBool,
}

impl PendingViewport {
    pub(super) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

pub(super) fn request(webview: Webview, pending: Arc<PendingViewport>, change: Change) {
    if pending.closed.load(Ordering::Acquire) {
        return;
    }
    if matches!(change, Change::NativeWindow) {
        pending.bounds_dirty.store(true, Ordering::Release);
    }
    pending.renderer_dirty.store(true, Ordering::Release);
    pending.dirty.store(true, Ordering::Release);
    if pending.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    let Ok(parent) = webview.window().hwnd() else {
        pending.queued.store(false, Ordering::Release);
        return;
    };
    let parent = parent.0 as usize;
    tauri::async_runtime::spawn(async move {
        // Consume the attempt before dispatch, including a dropped callback.
        // Only a fresh notification can request follow-up after failure.
        pending.dirty.store(false, Ordering::Release);
        let bounds = pending.bounds_dirty.swap(false, Ordering::AcqRel);
        let renderer = pending.renderer_dirty.swap(false, Ordering::AcqRel);
        let result = update(&webview, parent, pending.clone(), bounds, renderer).await;
        match result {
            Ok(Some(deferred_bounds)) => {
                if deferred_bounds {
                    pending.bounds_dirty.store(true, Ordering::Release);
                }
                pending.renderer_dirty.store(true, Ordering::Release);
            }
            Err(error) => debug!(?error, "main viewport reconciliation ended"),
            Ok(None) => {}
        }
        pending.queued.store(false, Ordering::Release);
        // Deferred minimized work alone does not schedule another attempt.
        if pending.dirty.swap(false, Ordering::AcqRel) {
            request(webview, pending, Change::RendererWindow);
        }
    });
}

async fn update(
    webview: &Webview,
    parent: usize,
    pending: Arc<PendingViewport>,
    bounds: bool,
    renderer: bool,
) -> Result<Option<bool>, String> {
    let lifecycle = pending.clone();
    let visible = on_controller(webview, move |controller| {
        if lifecycle.closed.load(Ordering::Acquire) {
            return Ok(false);
        }
        reconcile(HWND(parent as _), &controller, bounds)
    })
    .await?;
    if !visible {
        return Ok(Some(bounds));
    }
    if !renderer || pending.closed.load(Ordering::Acquire) {
        return Ok(None);
    }
    let observation = on_controller(webview, |controller| super::renderer::observe(&controller))
        .await?
        .await
        .map_err(|error| error.to_string())??;
    let correction = on_controller(webview, move |controller| {
        if pending.closed.load(Ordering::Acquire) {
            return Ok(super::renderer::Correction::Unchanged);
        }
        super::renderer::correct(HWND(parent as _), &controller, observation)
    })
    .await?;
    match correction {
        super::renderer::Correction::Deferred => Ok(Some(false)),
        super::renderer::Correction::Unchanged => Ok(None),
        super::renderer::Correction::Issued(completion) => {
            // Keep the worker occupied until actual COM completion/destruction.
            completion.await.map_err(|error| error.to_string())??;
            Ok(None)
        }
    }
}

async fn on_controller<T, F>(webview: &Webview, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(ICoreWebView2Controller) -> windows::core::Result<T> + Send + 'static,
{
    let (send, receive) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |platform| {
            let _ = send.send(task(platform.controller()).map_err(|error| error.to_string()));
        })
        .map_err(|error| error.to_string())?;
    receive.await.map_err(|error| error.to_string())?
}

fn reconcile(
    parent: HWND,
    controller: &ICoreWebView2Controller,
    bounds: bool,
) -> windows::core::Result<bool> {
    unsafe {
        if IsIconic(parent).as_bool() {
            return Ok(false);
        }
        let mut client = RECT::default();
        GetClientRect(parent, &mut client)?;
        if client.right <= 0 || client.bottom <= 0 {
            return Ok(false);
        }
        // Child-geometry observations are read-only unless actual drift exists.
        // Reapplying parent-position notifications here would feed the hook.
        if bounds {
            controller.SetBounds(client)?;
            controller.NotifyParentWindowPositionChanged()?;
        }
        Ok(true)
    }
}
