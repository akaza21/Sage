use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{ResourceRef, editor_notice_kind_from_unix_delivery, emit_if_active};
use crate::{
    error::{AppError, AppResult},
    workspace::notify::unix::{prepare_watch, run_prepared},
};

pub(super) fn spawn_editor_watch(
    app: tauri::AppHandle,
    resource: ResourceRef,
    path: PathBuf,
    generation: u32,
    cancelled: Arc<AtomicBool>,
) -> AppResult<()> {
    let directory = path.parent().map(Path::to_path_buf).ok_or_else(|| {
        AppError::InvalidRequest("editor watch path has no parent directory".into())
    })?;
    let watched_file_name = path
        .file_name()
        .ok_or_else(|| AppError::InvalidRequest("editor watch path has no file name".into()))?
        .to_string_lossy()
        .into_owned();
    let prepared = prepare_watch(std::slice::from_ref(&directory)).map_err(AppError::Io)?;
    std::thread::Builder::new()
        .name("sage-editor-watch".into())
        .spawn(move || {
            let result = run_prepared(prepared, &cancelled, &mut |relative, action| {
                if let Some(kind) =
                    editor_notice_kind_from_unix_delivery(&watched_file_name, &relative, action)
                {
                    emit_if_active(&app, &resource, generation, &cancelled, kind);
                }
            });
            if result.is_err()
                && let Some(kind) =
                    super::editor_watch_failure_resync(cancelled.load(Ordering::Acquire))
            {
                emit_if_active(&app, &resource, generation, &cancelled, kind);
            }
            super::finish_watch(&resource, generation);
        })
        .map(|_| ())
        .map_err(AppError::Io)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::editor_watch::EditorExternalWatchKind;
    use std::{
        fs,
        sync::{atomic::Ordering, mpsc},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn unix_editor_watch_reports_change_or_overflow_then_cancels() {
        let directory = std::env::temp_dir().join(format!(
            "sage-editor-watch-unix-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ));
        fs::create_dir(&directory).expect("create test directory");
        let watched_path = directory.join("notes.md");
        fs::write(&watched_path, "initial").expect("create watched file");

        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_directory = directory.clone();
        let (notices, receive_notice) = mpsc::sync_channel(4);
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = (|| {
                let prepared = prepare_watch(std::slice::from_ref(&worker_directory))?;
                run_prepared(prepared, &worker_cancelled, &mut |relative, action| {
                    if let Some(kind) =
                        editor_notice_kind_from_unix_delivery("notes.md", &relative, action)
                    {
                        notices
                            .send(kind)
                            .expect("test receives editor watch notice");
                    }
                })
            })();
            completed
                .send(result)
                .expect("test receives worker completion");
        });

        let mut saw_notice = false;
        for index in 0..20 {
            fs::write(&watched_path, format!("changed-{index}")).expect("modify watched file");
            if receive_notice
                .recv_timeout(Duration::from_millis(150))
                .is_ok_and(|kind| {
                    kind == EditorExternalWatchKind::Changed
                        || kind == EditorExternalWatchKind::Deleted
                })
            {
                saw_notice = true;
                break;
            }
        }
        assert!(
            saw_notice,
            "unix editor watch should report a named change or overflow resync"
        );

        cancelled.store(true, Ordering::Release);
        receive_completion
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled watch should stop promptly")
            .expect("cancelled watch should not fail");
        worker.join().expect("watch worker should not panic");
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
