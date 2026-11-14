use std::{
    fs::OpenOptions,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use sage_core::error::{AppError, AppResult};
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

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;
const ERROR_NOTIFY_ENUM_DIR: i32 = 1022;

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
            run_editor_watch(&cancelled, &directory, &watched_file_name, |kind| {
                emit_if_active(&app, &resource, generation, &cancelled, kind)
            });
            super::finish_watch(&resource, generation);
        })
        .map(|_| ())
        .map_err(AppError::Io)
}

fn run_editor_watch(
    cancelled: &AtomicBool,
    directory: &Path,
    watched_file_name: &str,
    mut deliver: impl FnMut(EditorExternalWatchKind),
) {
    let result = run_directory_watch(cancelled, directory, watched_file_name, &mut deliver);
    deliver_failure_resync(&result, cancelled.load(Ordering::Acquire), deliver);
}

fn deliver_failure_resync(
    result: &std::io::Result<()>,
    cancelled: bool,
    mut deliver: impl FnMut(EditorExternalWatchKind),
) {
    if result.is_err()
        && let Some(kind) = failure_resync(cancelled)
    {
        deliver(kind);
    }
}

/// Matches `editor_watch_failure_resync` on macOS and Linux, which is not
/// compiled into Windows builds.
fn failure_resync(cancelled: bool) -> Option<EditorExternalWatchKind> {
    (!cancelled).then_some(EditorExternalWatchKind::Changed)
}

fn run_directory_watch(
    cancelled: &AtomicBool,
    directory: &Path,
    watched_file_name: &str,
    deliver: impl FnMut(EditorExternalWatchKind),
) -> std::io::Result<()> {
    watch_directory(cancelled, directory, watched_file_name, deliver, || {})
}

fn watch_directory(
    cancelled: &AtomicBool,
    directory: &Path,
    watched_file_name: &str,
    mut deliver: impl FnMut(EditorExternalWatchKind),
    mut on_armed: impl FnMut(),
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
        on_armed();
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
        let last_error = (completed == 0)
            .then(|| std::io::Error::last_os_error().raw_os_error())
            .flatten();
        if !handle_watch_completion(
            WatchCompletion {
                completed,
                received,
                last_error,
                cancelled: cancelled.load(Ordering::Acquire),
            },
            &buffer,
            watched_file_name,
            &mut deliver,
        )? {
            return Ok(());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct WatchCompletion {
    completed: i32,
    received: u32,
    last_error: Option<i32>,
    cancelled: bool,
}

/// When more changes arrive than the buffer holds, Windows drops them and
/// completes the read with 0 bytes, or fails it with `ERROR_NOTIFY_ENUM_DIR`.
/// Either way the watched file may have changed, so this delivers `Changed`
/// and keeps watching, as the workspace notifier does.
fn handle_watch_completion(
    completion: WatchCompletion,
    buffer: &[u8],
    watched_file_name: &str,
    deliver: &mut impl FnMut(EditorExternalWatchKind),
) -> std::io::Result<bool> {
    if completion.cancelled {
        return Ok(false);
    }
    if overflow_watch_completion(
        completion.completed,
        completion.received,
        completion.last_error,
    ) {
        deliver(EditorExternalWatchKind::Changed);
        return Ok(true);
    }
    if completion.completed == 0 {
        return Err(completion
            .last_error
            .map_or_else(std::io::Error::last_os_error, |code| {
                std::io::Error::from_raw_os_error(code)
            }));
    }
    let received = usize::try_from(completion.received)
        .expect("Windows byte count fits usize")
        .min(buffer.len());
    if let Some(kind) = directory_notice_kind(buffer, received, watched_file_name) {
        deliver(kind);
    }
    Ok(true)
}

fn overflow_watch_completion(completed: i32, received: u32, last_error: Option<i32>) -> bool {
    if completed != 0 {
        return received == 0;
    }
    last_error == Some(ERROR_NOTIFY_ENUM_DIR)
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

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_OPERATION_ABORTED: i32 = 995;

    fn unique_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sage-editor-watch-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ))
    }

    fn completion(
        completed: i32,
        received: u32,
        last_error: Option<i32>,
        cancelled: bool,
    ) -> WatchCompletion {
        WatchCompletion {
            completed,
            received,
            last_error,
            cancelled,
        }
    }

    fn handle(
        completion: WatchCompletion,
        buffer: &[u8],
    ) -> (std::io::Result<bool>, Vec<EditorExternalWatchKind>) {
        let mut delivered = Vec::new();
        let result = handle_watch_completion(completion, buffer, "notes.md", &mut |kind| {
            delivered.push(kind)
        });
        (result, delivered)
    }

    #[test]
    fn zero_byte_completion_delivers_changed_once_and_keeps_watching() {
        // A stale record left in the buffer must not be read: 0 bytes means
        // Windows dropped this burst of changes.
        let mut buffer = [0_u8; 8_192];
        let stale = change(FILE_ACTION_REMOVED, "notes.md");
        buffer[..stale.len()].copy_from_slice(&stale);

        let (result, delivered) = handle(completion(1, 0, None, false), &buffer);

        assert!(result.expect("an overflow is not a watch failure"));
        assert_eq!(delivered, vec![EditorExternalWatchKind::Changed]);
    }

    #[test]
    fn notify_enum_dir_failure_delivers_changed_and_keeps_watching() {
        let buffer = [0_u8; 8_192];

        let (result, delivered) = handle(
            completion(0, 0, Some(ERROR_NOTIFY_ENUM_DIR), false),
            &buffer,
        );

        assert!(result.expect("ERROR_NOTIFY_ENUM_DIR is not a watch failure"));
        assert_eq!(delivered, vec![EditorExternalWatchKind::Changed]);
    }

    #[test]
    fn other_completion_failure_ends_the_watch_with_its_error() {
        let buffer = [0_u8; 8_192];

        let (result, delivered) =
            handle(completion(0, 0, Some(ERROR_ACCESS_DENIED), false), &buffer);

        assert_eq!(
            result
                .expect_err("a failed read ends the watch")
                .raw_os_error(),
            Some(ERROR_ACCESS_DENIED)
        );
        assert_eq!(delivered, Vec::new());
    }

    #[test]
    fn cancelled_completion_stops_without_delivering() {
        let records = change(FILE_ACTION_MODIFIED, "notes.md");
        let received = u32::try_from(records.len()).expect("record length fits u32");
        for completion in [
            completion(0, 0, Some(ERROR_OPERATION_ABORTED), true),
            completion(0, 0, Some(ERROR_NOTIFY_ENUM_DIR), true),
            completion(1, 0, None, true),
            completion(1, received, None, true),
        ] {
            let (result, delivered) = handle(completion, &records);
            assert!(
                !result.expect("a cancelled watch is not a failure"),
                "{completion:?} should stop the watch"
            );
            assert_eq!(delivered, Vec::new(), "{completion:?} delivered a notice");
        }
    }

    #[test]
    fn completed_records_deliver_only_the_watched_file() {
        let records = batch(&[
            (FILE_ACTION_MODIFIED, "sibling.md"),
            (FILE_ACTION_REMOVED, "notes.md"),
        ]);
        let received = u32::try_from(records.len()).expect("record length fits u32");

        let (result, delivered) = handle(completion(1, received, None, false), &records);
        assert!(result.expect("records keep the watch running"));
        assert_eq!(delivered, vec![EditorExternalWatchKind::Deleted]);

        let sibling = change(FILE_ACTION_MODIFIED, "sibling.md");
        let received = u32::try_from(sibling.len()).expect("record length fits u32");
        let (result, delivered) = handle(completion(1, received, None, false), &sibling);
        assert!(result.expect("records keep the watch running"));
        assert_eq!(delivered, Vec::new());
    }

    #[test]
    fn failure_resync_matches_the_unix_watch() {
        for cancelled in [false, true] {
            assert_eq!(
                failure_resync(cancelled),
                crate::editor_watch::editor_watch_failure_resync(cancelled)
            );
        }
    }

    #[test]
    fn watch_failure_delivers_changed_unless_cancelled() {
        let failed: std::io::Result<()> =
            Err(std::io::Error::from_raw_os_error(ERROR_ACCESS_DENIED));

        let mut delivered = Vec::new();
        deliver_failure_resync(&failed, false, |kind| delivered.push(kind));
        assert_eq!(delivered, vec![EditorExternalWatchKind::Changed]);

        let mut delivered = Vec::new();
        deliver_failure_resync(&failed, true, |kind| delivered.push(kind));
        assert_eq!(delivered, Vec::new());

        let mut delivered = Vec::new();
        deliver_failure_resync(&Ok(()), false, |kind| delivered.push(kind));
        assert_eq!(delivered, Vec::new());
    }

    #[test]
    fn a_watch_that_cannot_open_its_folder_delivers_changed_unless_cancelled() {
        let missing = unique_directory("missing");
        assert!(!missing.exists(), "test folder must not exist");

        let mut delivered = Vec::new();
        run_editor_watch(&AtomicBool::new(false), &missing, "notes.md", |kind| {
            delivered.push(kind)
        });
        assert_eq!(delivered, vec![EditorExternalWatchKind::Changed]);

        let mut delivered = Vec::new();
        run_editor_watch(&AtomicBool::new(true), &missing, "notes.md", |kind| {
            delivered.push(kind)
        });
        assert_eq!(delivered, Vec::new());
    }

    #[derive(Debug, PartialEq, Eq)]
    enum WatchEvent {
        Armed,
        Notice(EditorExternalWatchKind),
    }

    #[test]
    fn a_burst_that_overflows_the_watch_buffer_reports_changed_and_keeps_watching() {
        let directory = unique_directory("overflow");
        fs::create_dir(&directory).expect("create test directory");
        fs::write(directory.join("notes.md"), "initial").expect("create watched file");

        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_directory = directory.clone();
        let (events, receive_event) = mpsc::channel();
        let (resume, resume_watch) = mpsc::channel::<()>();
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            // The first armed read holds the watch thread, as a slow consumer
            // would. That read completes on the first change of the burst, and
            // the rest piles up in the 8 KiB buffer with no read to take it.
            let mut hold = Some(resume_watch);
            let result = watch_directory(
                &worker_cancelled,
                &worker_directory,
                "notes.md",
                |kind| {
                    events
                        .send(WatchEvent::Notice(kind))
                        .expect("test receives watch notice");
                },
                || {
                    events
                        .send(WatchEvent::Armed)
                        .expect("test receives armed signal");
                    if let Some(hold) = hold.take() {
                        hold.recv_timeout(Duration::from_secs(5))
                            .expect("overflow test should resume the held watch after the burst");
                    }
                },
            );
            completed
                .send(result)
                .expect("test receives worker completion");
        });
        let next_event = || {
            receive_event
                .recv_timeout(Duration::from_secs(5))
                .expect("watch should report its next step")
        };

        assert_eq!(next_event(), WatchEvent::Armed, "watch should be listening");

        // Each record is over 200 bytes, so 1,000 of them hold far more than
        // the 8 KiB buffer. None names the watched file, so only the overflow
        // can report `Changed`.
        let padding = "x".repeat(96);
        for index in 0..1_000 {
            fs::File::create(directory.join(format!("burst-{index:04}-{padding}.md")))
                .expect("create burst file");
        }
        resume.send(()).expect("watch waits to be resumed");

        let mut notices = Vec::new();
        loop {
            match next_event() {
                WatchEvent::Notice(kind) => notices.push(kind),
                WatchEvent::Armed if !notices.is_empty() => break,
                WatchEvent::Armed => {}
            }
        }
        assert_eq!(notices, vec![EditorExternalWatchKind::Changed]);

        cancelled.store(true, Ordering::Release);
        receive_completion
            .recv_timeout(Duration::from_secs(5))
            .expect("overflow test watch should stop after cancellation")
            .expect("cancelled watch should not fail");
        worker.join().expect("watch worker should not panic");
        fs::remove_dir_all(directory).expect("remove test directory");
    }

    #[test]
    fn directory_watch_reports_target_changes_and_deletion_then_cancels() {
        let directory = unique_directory("changes");
        fs::create_dir(&directory).expect("create test directory");
        let watched_path = directory.join("notes.md");
        fs::write(&watched_path, "initial").expect("create watched file");

        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_directory = directory.clone();
        let (notices, receive_notice) = mpsc::sync_channel(8);
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

        let quiet_deadline = std::time::Instant::now() + Duration::from_millis(300);
        while std::time::Instant::now() < quiet_deadline {
            let _ = receive_notice.recv_timeout(Duration::from_millis(50));
        }

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
