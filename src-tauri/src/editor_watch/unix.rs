use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use sage_core::error::{AppError, AppResult};

use super::{ResourceRef, editor_notice_kind_from_unix_delivery, emit_if_active};
use crate::workspace::notify::unix::{prepare_editor_watch, run_prepared};

pub(super) fn spawn_editor_watch(
    app: tauri::AppHandle,
    resource: ResourceRef,
    path: PathBuf,
    generation: u32,
    cancelled: Arc<AtomicBool>,
) -> AppResult<()> {
    let watched_file_name = path
        .file_name()
        .ok_or_else(|| AppError::InvalidRequest("editor watch path has no file name".into()))?
        .to_string_lossy()
        .into_owned();
    let prepared = prepare_editor_watch(&path).map_err(AppError::Io)?;
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

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;
