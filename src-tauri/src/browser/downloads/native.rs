use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::Path,
    sync::mpsc,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, Webview};
use tauri_plugin_dialog::DialogExt;
use webview2_com::{
    BytesReceivedChangedEventHandler, CoTaskMemPWSTR, DownloadStartingEventHandler,
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_DOWNLOAD_STATE, COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED,
        COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, ICoreWebView2_4, ICoreWebView2Deferral,
        ICoreWebView2DownloadOperation, ICoreWebView2DownloadStartingEventArgs,
    },
    StateChangedEventHandler,
};
use windows_core::{HSTRING, Interface, PWSTR};

use super::{BrowserState, DownloadView};

mod choice;
mod watchdog;

struct PendingDownload {
    args: ICoreWebView2DownloadStartingEventArgs,
    deferral: ICoreWebView2Deferral,
    operation: ICoreWebView2DownloadOperation,
    timer_id: usize,
    armed_at: Instant,
    app: AppHandle,
    tab_id: String,
}

thread_local! {
    // WebView2 interfaces are apartment-bound and must never enter BrowserState.
    static PENDING: RefCell<BTreeMap<u64, PendingDownload>> = const { RefCell::new(BTreeMap::new()) };
    static ACTIVE: RefCell<BTreeMap<u64, ICoreWebView2DownloadOperation>> = const { RefCell::new(BTreeMap::new()) };
}

fn emit_views(app: &AppHandle, tab_id: &str) {
    if let Some(webview) = app.state::<BrowserState>().webview_for_tab(tab_id) {
        crate::browser::background::refresh(&webview);
    }
    let views: Option<Vec<DownloadView>> = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .ok()
        .map(|registry| registry.views(tab_id));
    if let Some(downloads) = views {
        let _ = app.emit_to(
            "main",
            "browser:downloads",
            serde_json::json!({
                "tabId": tab_id,
                "downloads": downloads,
            }),
        );
    }
}

pub(crate) async fn register_native(app: AppHandle, webview: Webview) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let tab_id = webview.label().to_owned();
    let callback_webview = webview.clone();
    webview
        .with_webview(move |platform| {
            let result: Result<(), String> = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|error| error.to_string())?;
                let core: ICoreWebView2_4 = core.cast().map_err(|error| error.to_string())?;
                let mut token = 0;
                unsafe {
                    core.add_DownloadStarting(
                        &DownloadStartingEventHandler::create(Box::new(move |_, args| {
                            if let Some(args) = args {
                                start_download(&app, &callback_webview, &tab_id, args)?;
                            }
                            Ok(())
                        })),
                        &mut token,
                    )
                }
                .map_err(|error| error.to_string())
            })();
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?
}

fn start_download(
    app: &AppHandle,
    webview: &Webview,
    tab_id: &str,
    args: ICoreWebView2DownloadStartingEventArgs,
) -> windows_core::Result<()> {
    let prepared = (|| {
        let operation = unsafe { args.DownloadOperation()? };
        let mut source = PWSTR::null();
        unsafe { operation.Uri(&mut source)? };
        let source = CoTaskMemPWSTR::from(source).to_string();
        let mut suggested = PWSTR::null();
        unsafe { args.ResultFilePath(&mut suggested)? };
        Ok::<_, windows_core::Error>((
            operation,
            source,
            CoTaskMemPWSTR::from(suggested).to_string(),
        ))
    })();
    let (operation, source, suggested) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = unsafe { args.SetCancel(true) };
            let _ = unsafe { args.SetHandled(true) };
            return Err(error);
        }
    };
    // Diagnostic trace only: a navigation that became a download commits
    // nothing, so it settles as abandoned (`navigation::outcome`).
    if let Ok(mut tabs) = app.state::<BrowserState>().tabs.lock() {
        tabs.note_download(tab_id);
    }
    let file_name = Path::new(&suggested)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download")
        .to_owned();
    let id = app
        .state::<BrowserState>()
        .downloads
        .lock()
        .ok()
        .and_then(|mut registry| registry.begin(tab_id, &source, &file_name).ok());
    let Some(id) = id else {
        unsafe {
            args.SetCancel(true)?;
            args.SetHandled(true)?;
        }
        return Ok(());
    };
    let deferral = match unsafe { args.GetDeferral() } {
        Ok(deferral) => deferral,
        Err(error) => {
            if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
                registry.settle(id, "error");
            }
            unsafe {
                args.SetCancel(true)?;
                args.SetHandled(true)?;
            }
            emit_views(app, tab_id);
            return Err(error);
        }
    };
    let Some((timer_id, armed_at)) = watchdog::arm() else {
        if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
            registry.settle(id, "error");
        }
        let _ = unsafe { args.SetCancel(true) };
        let _ = unsafe { operation.Cancel() };
        let _ = unsafe { deferral.Complete() };
        emit_views(app, tab_id);
        return Err(windows_core::Error::from_win32());
    };
    PENDING.with_borrow_mut(|pending| {
        pending.insert(
            id,
            PendingDownload {
                args,
                deferral,
                operation,
                timer_id,
                armed_at,
                app: app.clone(),
                tab_id: tab_id.to_owned(),
            },
        );
    });
    emit_views(app, tab_id);

    let picker_app = app.clone();
    let picker_webview = webview.clone();
    let picker_tab = tab_id.to_owned();
    app.dialog()
        .file()
        .set_title("Save browser download")
        .set_file_name(file_name)
        .save_file(move |selected| {
            let path = selected.and_then(|file| file.into_path().ok());
            let dispatch_app = picker_app.clone();
            let dispatch_tab = picker_tab.clone();
            if picker_webview
                .with_webview(move |_| {
                    complete_choice(&dispatch_app, &dispatch_tab, id, path);
                })
                .is_err()
            {
                let fallback_app = picker_app.clone();
                let fallback_tab = picker_tab.clone();
                if let Err(error) = picker_app.run_on_main_thread(move || {
                    complete_choice(&fallback_app, &fallback_tab, id, None);
                }) {
                    if let Ok(mut registry) = picker_app.state::<BrowserState>().downloads.lock() {
                        registry.settle(id, "error");
                    }
                    emit_views(&picker_app, &picker_tab);
                    eprintln!("browser download picker dispatch failed: {error}");
                }
            }
        });
    Ok(())
}

fn complete_choice(app: &AppHandle, tab_id: &str, id: u64, path: Option<std::path::PathBuf>) {
    let pending = PENDING.with_borrow_mut(|downloads| downloads.remove(&id));
    if let Some(pending) = pending {
        watchdog::disarm(pending.timer_id);
        let selected =
            path.and_then(|path| path.to_str().map(|text| (path.clone(), text.to_owned())));
        if let Some((path, path_text)) = selected {
            let allowed = app
                .state::<BrowserState>()
                .downloads
                .lock()
                .is_ok_and(|mut registry| {
                    choice::accept_destination(
                        &mut registry,
                        id,
                        path,
                        pending.armed_at,
                        Instant::now(),
                    )
                });
            if allowed {
                let configured =
                    unsafe { pending.args.SetResultFilePath(&HSTRING::from(path_text)) }
                        .and_then(|()| unsafe { pending.args.SetHandled(true) })
                        .is_ok();
                if configured {
                    if install_progress(app.clone(), tab_id.to_owned(), id, &pending.operation)
                        .is_ok()
                    {
                        ACTIVE.with_borrow_mut(|active| {
                            active.insert(id, pending.operation);
                        });
                    } else {
                        if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
                            registry.settle(id, "error");
                        }
                        let _ = unsafe { pending.args.SetCancel(true) };
                        let _ = unsafe { pending.operation.Cancel() };
                    }
                } else {
                    if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
                        registry.settle(id, "error");
                    }
                    let _ = unsafe { pending.args.SetCancel(true) };
                }
            } else {
                let _ = unsafe { pending.args.SetCancel(true) };
            }
        } else {
            let _ = app
                .state::<BrowserState>()
                .downloads
                .lock()
                .map(|mut registry| {
                    registry.settle(id, "canceled");
                });
            let _ = unsafe { pending.args.SetCancel(true) };
        }
        let _ = unsafe { pending.deferral.Complete() };
    }
    emit_views(app, tab_id);
}

fn install_progress(
    app: AppHandle,
    tab_id: String,
    id: u64,
    operation: &ICoreWebView2DownloadOperation,
) -> windows_core::Result<()> {
    let progress_app = app.clone();
    let progress_tab = tab_id.clone();
    let mut token = 0;
    unsafe {
        operation.add_BytesReceivedChanged(
            &BytesReceivedChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut received = 0;
                    let mut total = 0;
                    sender.BytesReceived(&mut received)?;
                    sender.TotalBytesToReceive(&mut total)?;
                    if let Ok(mut registry) = progress_app.state::<BrowserState>().downloads.lock()
                    {
                        registry.progress(
                            id,
                            received.max(0) as u64,
                            (total >= 0).then_some(total as u64),
                        );
                    }
                    emit_views(&progress_app, &progress_tab);
                }
                Ok(())
            })),
            &mut token,
        )
    }?;
    let mut token = 0;
    unsafe {
        operation.add_StateChanged(
            &StateChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(sender) = sender {
                    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
                    sender.State(&mut state)?;
                    if state != COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS {
                        let terminal = if state == COREWEBVIEW2_DOWNLOAD_STATE_COMPLETED {
                            "completed"
                        } else {
                            "interrupted"
                        };
                        let mut raw = PWSTR::null();
                        let saved = sender
                            .ResultFilePath(&mut raw)
                            .ok()
                            .map(|()| CoTaskMemPWSTR::from(raw).to_string());
                        if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
                            if let Some(saved) = saved.filter(|saved| !saved.is_empty()) {
                                registry.saved_as(id, Path::new(&saved));
                            }
                            registry.settle(id, terminal);
                        }
                        ACTIVE.with_borrow_mut(|active| {
                            active.remove(&id);
                        });
                        emit_views(&app, &tab_id);
                    }
                }
                Ok(())
            })),
            &mut token,
        )
    }?;
    Ok(())
}

pub(crate) fn cancel_native(webview: &Webview, ids: Vec<u64>) -> Result<(), String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |_| {
            let mut result = Ok(());
            for id in ids {
                if let Some(pending) = PENDING.with_borrow_mut(|entries| entries.remove(&id)) {
                    watchdog::disarm(pending.timer_id);
                    let canceled = unsafe { pending.args.SetCancel(true) }.is_ok()
                        || unsafe { pending.operation.Cancel() }.is_ok();
                    let completed = unsafe { pending.deferral.Complete() }.is_ok();
                    if !canceled || !completed {
                        result = Err("browser download cancellation failed".into());
                    }
                } else if let Some(operation) =
                    ACTIVE.with_borrow(|entries| entries.get(&id).cloned())
                {
                    if unsafe { operation.Cancel() }.is_err() {
                        result = Err("browser download cancellation failed".into());
                    }
                } else {
                    result = Err("browser download unavailable".into());
                }
            }
            let _ = sender.send(result);
        })
        .map_err(|error| error.to_string())?;
    receiver
        .recv_timeout(Duration::from_secs(5))
        .map_err(|error| error.to_string())?
}
