//! The second half of a download start: plan on the download worker, then
//! save silently or show the Save dialog back on the WebView2 UI thread.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Webview};
use tauri_plugin_dialog::DialogExt;

use super::{
    super::{BrowserState, worker},
    PENDING, RESERVED,
    choice::{self, Step},
    complete_choice, emit_views,
};

pub(super) fn plan_off_thread(
    app: &AppHandle,
    webview: &Webview,
    tab_id: &str,
    id: u64,
    file_name: String,
    mime: Option<String>,
) {
    let worker_app = app.clone();
    let worker_webview = webview.clone();
    let worker_tab = tab_id.to_owned();
    let queued = worker::submit(move || {
        // A reserved placeholder is owned by `planned` from here on: if the
        // hand-back below never runs, dropping it releases the file.
        let planned = choice::plan_download(&worker_app, &file_name, mime.as_deref());
        let ui_app = worker_app.clone();
        let ui_webview = worker_webview.clone();
        let ui_tab = worker_tab.clone();
        on_ui(&worker_app, &worker_webview, &worker_tab, id, move || {
            proceed(&ui_app, &ui_webview, &ui_tab, id, planned);
        });
    });
    if !queued {
        // The worker is busy with as many downloads as it queues: cancel.
        complete_choice(app, tab_id, id, None);
    }
}

/// Runs `work` on the WebView2 UI thread, where `PENDING` lives. If the
/// webview is gone, the download is canceled from the main thread instead.
fn on_ui(
    app: &AppHandle,
    webview: &Webview,
    tab_id: &str,
    id: u64,
    work: impl FnOnce() + Send + 'static,
) {
    if webview.with_webview(move |_| work()).is_ok() {
        return;
    }
    let fallback_app = app.clone();
    let fallback_tab = tab_id.to_owned();
    if let Err(error) = app.run_on_main_thread(move || {
        complete_choice(&fallback_app, &fallback_tab, id, None);
    }) {
        if let Ok(mut registry) = app.state::<BrowserState>().downloads.lock() {
            registry.settle(id, "error");
        }
        emit_views(app, tab_id);
        eprintln!("browser download dispatch failed: {error}");
    }
}

fn proceed(app: &AppHandle, webview: &Webview, tab_id: &str, id: u64, planned: choice::Planned) {
    let choice::Planned { step, revision } = planned;
    let remember = matches!(step, Step::Ask { remember: true, .. });
    let still_pending = PENDING.with_borrow_mut(|pending| {
        pending.get_mut(&id).is_some_and(|download| {
            if remember {
                download.remember = Some(revision);
            }
            true
        })
    });
    if !still_pending {
        // Canceled while planning (Cancel, tab close, watchdog).
        complete_choice(app, tab_id, id, None);
        return;
    }
    match step {
        Step::Silent(mut reservation) => {
            let path = reservation.path().to_owned();
            // Claim while the placeholder still exists. Other downloads must
            // avoid this name even after its file is retired on the worker.
            let claimed = app
                .state::<BrowserState>()
                .downloads
                .lock()
                .is_ok_and(|mut registry| registry.record_destination(id, path.clone()));
            if !claimed {
                complete_choice(app, tab_id, id, None);
                return;
            }
            let worker_app = app.clone();
            let worker_webview = webview.clone();
            let worker_tab = tab_id.to_owned();
            let queued = worker::submit(move || {
                let ready = reservation.handoff();
                let ui_app = worker_app.clone();
                let ui_tab = worker_tab.clone();
                on_ui(&worker_app, &worker_webview, &worker_tab, id, move || {
                    RESERVED.with_borrow_mut(|reserved| reserved.insert(id, reservation));
                    // complete_choice rechecks the pending id and watchdog:
                    // cancellation while queued must never restart a download.
                    complete_choice(&ui_app, &ui_tab, id, ready.then_some(path));
                });
            });
            if !queued {
                complete_choice(app, tab_id, id, None);
            }
        }
        Step::Ask {
            file_name,
            directory,
            ..
        } => {
            open_save_dialog(app, webview, tab_id, id, file_name, directory);
        }
    }
}

fn open_save_dialog(
    app: &AppHandle,
    webview: &Webview,
    tab_id: &str,
    id: u64,
    file_name: String,
    directory: Option<PathBuf>,
) {
    let picker_app = app.clone();
    let picker_webview = webview.clone();
    let picker_tab = tab_id.to_owned();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Save browser download")
        .set_file_name(file_name);
    if let Some(directory) = directory {
        dialog = dialog.set_directory(directory);
    }
    dialog.save_file(move |selected| {
        let path = selected.and_then(|file| file.into_path().ok());
        let ui_app = picker_app.clone();
        let ui_tab = picker_tab.clone();
        on_ui(&picker_app, &picker_webview, &picker_tab, id, move || {
            complete_choice(&ui_app, &ui_tab, id, path);
        });
    });
}
