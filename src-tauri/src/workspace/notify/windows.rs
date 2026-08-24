use std::{
    fs::OpenOptions,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tauri::{AppHandle, Emitter};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
        FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    },
    System::Threading::WaitForSingleObject,
};

use super::{
    apply_unwatch_if_generation, deliver_workspace_notify_action, reject, WorkspaceNotifyError,
    WORKSPACE_NOTIFY_EVENT,
};

const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 258;

fn open_notify_directory(directory: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .open(directory)
}

pub(super) fn spawn_workspace_root_watch(
    app: AppHandle,
    project_id: String,
    generation: u32,
    root_id: String,
    path: PathBuf,
    cancelled: Arc<AtomicBool>,
) -> Result<(), WorkspaceNotifyError> {
    let directory_handle =
        open_notify_directory(&path).map_err(|_| reject("notify-substituted"))?;
    std::thread::Builder::new()
        .name("sage-workspace-notify".into())
        .spawn(move || {
            let result =
                watch_opened_directory(&cancelled, directory_handle, |relative, action| {
                    if cancelled.load(Ordering::Acquire) {
                        return;
                    }
                    if let Ok(Some(notice)) = deliver_workspace_notify_action(
                        &project_id,
                        generation,
                        &root_id,
                        &relative,
                        action,
                    ) {
                        let _ = app.emit(WORKSPACE_NOTIFY_EVENT, notice);
                    }
                });
            if result.is_err() && !cancelled.load(Ordering::Acquire) {
                let _ = apply_unwatch_if_generation(&project_id, generation);
            }
        })
        .map(|_| ())
        .map_err(|_| reject("notify-substituted"))
}

fn run_workspace_root_watch(
    cancelled: &AtomicBool,
    directory: &Path,
    deliver: impl FnMut(String, u32),
) -> std::io::Result<()> {
    watch_opened_directory(cancelled, open_notify_directory(directory)?, deliver)
}

fn watch_opened_directory(
    cancelled: &AtomicBool,
    directory_handle: std::fs::File,
    mut deliver: impl FnMut(String, u32),
) -> std::io::Result<()> {
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
                1,
                FILE_NOTIFY_CHANGE_FILE_NAME
                    | FILE_NOTIFY_CHANGE_DIR_NAME
                    | FILE_NOTIFY_CHANGE_LAST_WRITE,
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
        if cancelled.load(Ordering::Acquire) {
            return Ok(());
        }
        for (name, action) in parse_notify_records(
            &buffer,
            usize::try_from(received).expect("Windows byte count fits usize"),
        ) {
            if let Ok(relative) = super::relative_from_notify_name(&name) {
                deliver(relative, action);
            }
        }
    }
    Ok(())
}

fn parse_notify_records(buffer: &[u8], received: usize) -> Vec<(String, u32)> {
    let mut offset = 0_usize;
    let mut records = Vec::new();
    while offset.checked_add(12).is_some_and(|end| end <= received) {
        let Some(next) = buffer
            .get(offset..offset + 4)
            .and_then(|bytes| bytes.try_into().ok())
        else {
            return records;
        };
        let Some(action) = buffer
            .get(offset + 4..offset + 8)
            .and_then(|bytes| bytes.try_into().ok())
        else {
            return records;
        };
        let Some(name_len) = buffer
            .get(offset + 8..offset + 12)
            .and_then(|bytes| bytes.try_into().ok())
            .and_then(|bytes| usize::try_from(u32::from_le_bytes(bytes)).ok())
        else {
            return records;
        };
        let Some(name_end) = offset
            .checked_add(12)
            .and_then(|start| start.checked_add(name_len))
        else {
            return records;
        };
        if name_len % 2 != 0 || name_end > received {
            return records;
        }
        let name = (offset + 12..name_end)
            .step_by(2)
            .map(|index| u16::from_le_bytes([buffer[index], buffer[index + 1]]))
            .collect::<Vec<_>>();
        records.push((String::from_utf16_lossy(&name), u32::from_le_bytes(action)));
        let next = u32::from_le_bytes(next);
        if next == 0 {
            return records;
        }
        let Ok(next) = usize::try_from(next) else {
            return records;
        };
        if next < 12
            || offset
                .checked_add(next)
                .is_none_or(|value| value >= received)
        {
            return records;
        }
        offset += next;
    }
    records
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

    #[test]
    fn parse_notify_records_reads_action_and_relative_name() {
        let added = change(1, "src\\main.ts");
        assert_eq!(
            parse_notify_records(&added, added.len()),
            vec![("src\\main.ts".into(), 1)]
        );
    }

    #[test]
    fn workspace_root_watch_reports_create_then_cancels() {
        let directory = std::env::temp_dir().join(format!(
            "sage-workspace-notify-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time is after the Unix epoch")
                .as_nanos(),
        ));
        fs::create_dir(&directory).expect("create test directory");
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_directory = directory.clone();
        let (notices, receive_notice) = mpsc::sync_channel(4);
        let (completed, receive_completion) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = run_workspace_root_watch(
                &worker_cancelled,
                &worker_directory,
                |relative, action| {
                    notices
                        .send((relative, action))
                        .expect("test receives watch notice")
                },
            );
            completed
                .send(result)
                .expect("test receives worker completion");
        });

        let mut saw_create = false;
        for index in 0..20 {
            fs::write(directory.join(format!("notes-{index}.md")), "created")
                .expect("create watched file");
            if receive_notice
                .recv_timeout(Duration::from_millis(150))
                .is_ok_and(|(relative, action)| relative.starts_with("notes-") && action == 1)
            {
                saw_create = true;
                break;
            }
        }
        assert!(saw_create, "root file create should be reported");

        cancelled.store(true, Ordering::Release);
        receive_completion
            .recv_timeout(Duration::from_secs(2))
            .expect("cancelled watch should stop promptly")
            .expect("cancelled watch should not fail");
        worker.join().expect("watch worker should not panic");
        fs::remove_dir_all(directory).expect("remove test directory");
    }
}
