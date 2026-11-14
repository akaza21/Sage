use tauri::{AppHandle, Manager, RunEvent};
use tracing::{info, warn};

use crate::{browser::BrowserState, preview_lifecycle, state::AppState};

#[path = "exit_cleanup.rs"]
mod exit_cleanup;
use exit_cleanup::GIT_EXIT;

#[derive(Debug, PartialEq, Eq)]
enum ExitRequestOutcome {
    Allowed,
    AllowedAfterRetry(crate::acp::runtime::HostError),
    AllowedAfterFailedRetry {
        first_error: crate::acp::runtime::HostError,
        retry_error: crate::acp::runtime::HostError,
    },
    Deferred(crate::acp::runtime::HostError),
}

fn handle_exit_request(
    mut shutdown: impl FnMut() -> Result<(), crate::acp::runtime::HostError>,
    prevent_exit: impl FnOnce(),
    window_gone: bool,
) -> ExitRequestOutcome {
    let Err(first_error) = shutdown() else {
        return ExitRequestOutcome::Allowed;
    };
    if cfg!(windows) && window_gone {
        return match shutdown() {
            Ok(()) => ExitRequestOutcome::AllowedAfterRetry(first_error),
            Err(retry_error) => ExitRequestOutcome::AllowedAfterFailedRetry {
                first_error,
                retry_error,
            },
        };
    }
    prevent_exit();
    ExitRequestOutcome::Deferred(first_error)
}

pub(crate) fn handle_run_event(app_handle: &AppHandle, event: RunEvent) {
    if let RunEvent::WindowEvent { label, event, .. } = &event {
        crate::window_memory::handle_window_event(app_handle, label, event);
        #[cfg(windows)]
        {
            crate::window_frame::handle_window_event(app_handle, label, event);
        }
    }
    if matches!(
        &event,
        RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } if label == "main"
    ) || matches!(&event, RunEvent::Exit)
    {
        if !GIT_EXIT.finished() && !crate::workspace::notify::cancel_all_workspace_watches() {
            warn!("workspace watcher Git cleanup did not settle before window teardown");
            GIT_EXIT.start(app_handle.clone(), 0);
        }
        if let Err(error) = app_handle.state::<AppState>().terminals.shutdown() {
            warn!(?error, "terminal app-exit cleanup remains unsettled");
        }
        if matches!(&event, RunEvent::Exit)
            && app_handle
                .state::<AppState>()
                .acp
                .shutdown_blocking()
                .is_ok()
            && let Err(error) = app_handle.state::<AppState>().database.purge_side_chats()
        {
            warn!(
                ?error,
                "temporary Chat cleanup will retry before next startup"
            );
        }
        app_handle
            .state::<BrowserState>()
            .clear_teardown_after_window_destroy();
        app_handle
            .state::<crate::browser::passwords::PasswordsState>()
            .clear_clipboard_on_exit();
    }
    if let RunEvent::ExitRequested { api, code, .. } = event {
        info!("Sage host ExitRequested shutdown start");
        let window_gone = app_handle.get_webview_window("main").is_none();
        let state = app_handle.state::<AppState>();
        // An archive release still parking a runtime finishes first, within
        // the same budget the lanes' shutdown has.
        if !state
            .archive_releases
            .wait_settled(crate::acp::service::explicit_close_deadline())
        {
            warn!("an archived Chat's runtime release did not settle before exit");
        }
        preview_lifecycle::cleanup_previews_on_exit();
        let final_windowless_exit = window_gone && GIT_EXIT.finished();
        let outcome = handle_exit_request(
            || state.acp.shutdown_blocking(),
            || {
                if !final_windowless_exit {
                    api.prevent_exit();
                }
            },
            window_gone,
        );
        if !matches!(&outcome, ExitRequestOutcome::Deferred(_))
            && GIT_EXIT.defer(
                crate::workspace::notify::cancel_all_workspace_watches,
                || api.prevent_exit(),
                || GIT_EXIT.start(app_handle.clone(), code.unwrap_or(0)),
            )
        {
            warn!("Sage host exit deferred until workspace watcher Git cleanup settles");
            return;
        }
        match outcome {
            ExitRequestOutcome::Allowed => {
                info!("Sage host ExitRequested shutdown end; exit allowed");
            }
            ExitRequestOutcome::AllowedAfterRetry(first_error) => {
                warn!(shutdown_error = ?first_error, "Sage host shutdown succeeded on bounded retry; exit allowed");
            }
            ExitRequestOutcome::AllowedAfterFailedRetry {
                first_error,
                retry_error,
            } => {
                warn!(shutdown_error = ?first_error, retry_error = ?retry_error, "Sage host windowless exit allowed; owned Windows Job will terminate with host");
            }
            ExitRequestOutcome::Deferred(error) if final_windowless_exit => {
                warn!(shutdown_error = ?error, "Sage host final windowless exit allowed after bounded cleanup; remaining owned processes abandoned");
            }
            ExitRequestOutcome::Deferred(error) => {
                warn!(shutdown_error = ?error, "Sage host exit vetoed for bounded ACP recovery retry");
            }
        }
    }
}

#[cfg(test)]
#[path = "exit_tests.rs"]
mod exit_tests;
