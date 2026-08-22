use std::{
    fs::OpenOptions,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
    System::Threading::WaitForSingleObject,
};

use super::{EditorExternalWatchKind, ResourceRef, emit_if_active, notice_kind_for_path};
use crate::error::{AppError, AppResult};

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;

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
    std::thread::Builder::new()
        .name("sage-editor-watch".into())
        .spawn(move || {
            let _ = run_directory_watch(&cancelled, &directory, &watched_file_name, |kind| {
                emit_if_active(&app, &resource, generation, &cancelled, kind)
            });
            super::finish_watch(&resource, generation);
        })
        .map(|_| ())
        .map_err(AppError::Io)
}

fn run_directory_watch(
    cancelled: &AtomicBool,
    directory: &Path,
    watched_file_name: &str,
    mut deliver: impl FnMut(EditorExternalWatchKind),
) -> std::io::Result<()> {
    let directory_handle = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .open(directory)?;
    let event = EventHandle::new()?;
    let handle = directory_handle.as_raw_handle() as HANDLE;
    let mut buffer = [0_u8; 8_192];

    while !cancelled.load(Ordering::Acquire) {
        let mut overlapped = RawOverlapped::with_event(event.raw());
        let requested = unsafe {
            read_directory_changes(
                handle,
                buffer.as_mut_ptr().cast(),
                u32::try_from(buffer.len()).expect("watch buffer fits in u32"),
                0,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE,
                std::ptr::null_mut(),
                &mut overlapped,
            )
        };
        if requested == 0 && std::io::Error::last_os_error().raw_os_error() != Some(997) {
            return Err(std::io::Error::last_os_error());
        }
        loop {
            let result = unsafe { WaitForSingleObject(event.raw(), 100) };
            if cancelled.load(Ordering::Acquire) {
                unsafe { cancel_io(handle, &overlapped) };
            }
            if result == WAIT_OBJECT_0 || cancelled.load(Ordering::Acquire) {
                break;
            }
            if result != WAIT_TIMEOUT {
                return Err(std::io::Error::last_os_error());
            }
        }
        let mut received = 0_u32;
        let completed = unsafe { get_overlapped_result(handle, &overlapped, &mut received, 1) };
        if completed == 0 {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            return Err(std::io::Error::last_os_error());
        }
        let kind = directory_notice_kind(
            &buffer,
            usize::try_from(received).expect("Windows byte count fits usize"),
            watched_file_name,
        );
        if let Some(kind) = kind {
            deliver(kind);
        }
    }
    Ok(())
}

fn directory_notice_kind(
    buffer: &[u8],
    received: usize,
    watched_file_name: &str,
) -> Option<EditorExternalWatchKind> {
    let mut offset = 0_usize;
    let mut result = None;
    while offset.checked_add(12).is_some_and(|end| end <= received) {
        let next = u32::from_le_bytes(buffer[offset..offset + 4].try_into().ok()?);
        let action = u32::from_le_bytes(buffer[offset + 4..offset + 8].try_into().ok()?);
        let name_len = usize::try_from(u32::from_le_bytes(
            buffer[offset + 8..offset + 12].try_into().ok()?,
        ))
        .ok()?;
        let name_end = offset.checked_add(12)?.checked_add(name_len)?;
        if name_len % 2 != 0 || name_end > received {
            return result;
        }
        let name = (offset + 12..name_end)
            .step_by(2)
            .map(|index| u16::from_le_bytes([buffer[index], buffer[index + 1]]))
            .collect::<Vec<_>>();
        let changed_file_name = String::from_utf16_lossy(&name);
        if let Some(kind) = notice_kind_for_path(watched_file_name, &changed_file_name, action) {
            result = Some(kind);
        }
        if next == 0 {
            return result;
        }
        let next = usize::try_from(next).ok()?;
        if next < 12
            || offset
                .checked_add(next)
                .is_none_or(|value| value >= received)
        {
            return result;
        }
        offset += next;
    }
    result
}

#[repr(C)]
struct RawOverlapped {
    internal: usize,
    internal_high: usize,
    offset: [u32; 2],
    event: HANDLE,
}

impl RawOverlapped {
    fn with_event(event: HANDLE) -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: [0, 0],
            event,
        }
    }
}

struct EventHandle(HANDLE);

impl EventHandle {
    fn new() -> std::io::Result<Self> {
        let handle = unsafe {
            windows_sys::Win32::System::Threading::CreateEventW(
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
            )
        };
        if handle.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "ReadDirectoryChangesW"]
    fn read_directory_changes(
        directory: HANDLE,
        buffer: *mut std::ffi::c_void,
        buffer_len: u32,
        watch_subtree: i32,
        notify_filter: u32,
        bytes_returned: *mut u32,
        overlapped: *mut RawOverlapped,
    ) -> i32;
    #[link_name = "GetOverlappedResult"]
    fn get_overlapped_result(
        handle: HANDLE,
        overlapped: *const RawOverlapped,
        bytes_transferred: *mut u32,
        wait: i32,
    ) -> i32;
    #[link_name = "CancelIoEx"]
    fn cancel_io(handle: HANDLE, overlapped: *const RawOverlapped) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_watch::{FILE_ACTION_ADDED, FILE_ACTION_MODIFIED, FILE_ACTION_REMOVED};
    use std::{
        fs,
        sync::mpsc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    fn change(action: u32, name: &str) -> Vec<u8> {
        let name = name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let mut bytes = vec![0_u8; 12];
        bytes[4..8].copy_from_slice(&action.to_le_bytes());
        bytes[8..12].copy_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend(name);
        bytes
    }

    fn batch(changes: &[(u32, &str)]) -> Vec<u8> {
        let mut batch = Vec::new();
        for (index, (action, name)) in changes.iter().enumerate() {
            let mut notice = change(*action, name);
            if index + 1 < changes.len() {
                let notice_len = notice.len() as u32;
                notice[..4].copy_from_slice(&notice_len.to_le_bytes());
            }
            batch.extend(notice);
        }
        batch
    }

    #[test]
    fn directory_notifications_report_only_the_watched_file() {
        let changed = change(FILE_ACTION_MODIFIED, "notes.md");
        assert_eq!(
            directory_notice_kind(&changed, changed.len(), "notes.md"),
            Some(EditorExternalWatchKind::Changed)
        );
        let deleted = change(FILE_ACTION_REMOVED, "notes.md");
        assert_eq!(
            directory_notice_kind(&deleted, deleted.len(), "notes.md"),
            Some(EditorExternalWatchKind::Deleted)
        );
        let sibling = change(FILE_ACTION_MODIFIED, "sibling.md");
        assert_eq!(
            directory_notice_kind(&sibling, sibling.len(), "notes.md"),
            None
        );
    }

    #[test]
    fn deletion_followed_by_recreation_reports_changed() {
        let notices = batch(&[
            (FILE_ACTION_REMOVED, "notes.md"),
            (FILE_ACTION_ADDED, "notes.md"),
        ]);

        assert_eq!(
            directory_notice_kind(&notices, notices.len(), "notes.md"),
            Some(EditorExternalWatchKind::Changed)
        );
    }

    #[test]
    fn directory_watch_reports_target_changes_and_deletion_then_cancels() {
        let directory = std::env::temp_dir().join(format!(
            "sage-editor-watch-{}-{}",
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
        let (notices, receive_notice) = mpsc::sync_channel(1);
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result =
                run_directory_watch(&worker_cancelled, &worker_directory, "notes.md", |kind| {
                    notices.send(kind).expect("test receives watch notice")
                });
            completed
                .send(result)
                .expect("test receives worker completion");
        });

        let mut saw_change = false;
        for index in 0..20 {
            fs::write(&watched_path, format!("changed-{index}")).expect("modify watched file");
            if receive_notice
                .recv_timeout(Duration::from_millis(100))
                .is_ok_and(|kind| kind == EditorExternalWatchKind::Changed)
            {
                saw_change = true;
                break;
            }
        }
        assert!(saw_change, "watched file change should be reported");

        while receive_notice.try_recv().is_ok() {}

        fs::write(directory.join("sibling.md"), "sibling").expect("modify sibling file");
        assert!(
            receive_notice
                .recv_timeout(Duration::from_millis(200))
                .is_err(),
            "sibling changes must not be reported"
        );

        fs::remove_file(&watched_path).expect("delete watched file");
        assert_eq!(
            receive_notice
                .recv_timeout(Duration::from_secs(2))
                .expect("watched file deletion should be reported"),
            EditorExternalWatchKind::Deleted
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
