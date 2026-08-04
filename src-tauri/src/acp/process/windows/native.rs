use std::fs::File;
use std::mem::size_of;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, UpdateProcThreadAttribute,
};

use super::super::supervisor::LaunchError;

pub(super) struct NativeHandle(HANDLE);

// Windows kernel handles may be waited, queried, and closed from any thread;
// this wrapper owns exactly one handle and closes it once on drop.
unsafe impl Send for NativeHandle {}
unsafe impl Sync for NativeHandle {}

impl NativeHandle {
    pub(super) fn new(value: HANDLE) -> Result<Self, LaunchError> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(LaunchError::Io)
        } else {
            Ok(Self(value))
        }
    }

    pub(super) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for NativeHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: the wrapper uniquely owns this valid kernel handle.
            unsafe { CloseHandle(self.0) };
            self.0 = null_mut();
        }
    }
}

pub(super) struct AttributeList {
    storage: Vec<usize>,
    pub(super) pointer: LPPROC_THREAD_ATTRIBUTE_LIST,
}

impl AttributeList {
    pub(super) fn handles(handles: &[HANDLE; 3]) -> Result<Self, LaunchError> {
        let mut bytes = 0_usize;
        // SAFETY: the first call obtains the required allocation size.
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        let mut storage = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
        let pointer = storage.as_mut_ptr().cast();
        // SAFETY: storage is aligned and has the size returned above.
        if unsafe { InitializeProcThreadAttributeList(pointer, 1, 0, &mut bytes) } == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        // SAFETY: pointer is initialized and the value is exactly three HANDLEs.
        if unsafe {
            UpdateProcThreadAttribute(
                pointer,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                null_mut(),
                null(),
            )
        } == 0
        {
            // SAFETY: successful initialization must be paired with deletion.
            unsafe { DeleteProcThreadAttributeList(pointer) };
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(Self { storage, pointer })
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        let _ = self.storage.len();
        // SAFETY: pointer was successfully initialized and is uniquely owned.
        unsafe { DeleteProcThreadAttributeList(self.pointer) };
    }
}

pub(super) struct StdioSet {
    parent_stdin: File,
    parent_stdout: File,
    parent_stderr: File,
    child_stdin: NativeHandle,
    child_stdout: NativeHandle,
    child_stderr: NativeHandle,
}

impl StdioSet {
    pub(super) fn create() -> Result<Self, LaunchError> {
        let (child_stdin, parent_stdin) = pipe(false)?;
        let (child_stdout, parent_stdout) = pipe(true)?;
        let (child_stderr, parent_stderr) = pipe(true)?;
        Ok(Self {
            parent_stdin,
            parent_stdout,
            parent_stderr,
            child_stdin,
            child_stdout,
            child_stderr,
        })
    }

    pub(super) fn child_handles(&self) -> [HANDLE; 3] {
        [
            self.child_stdin.raw(),
            self.child_stdout.raw(),
            self.child_stderr.raw(),
        ]
    }

    pub(super) fn into_parent(self) -> (File, File, File) {
        let Self {
            parent_stdin,
            parent_stdout,
            parent_stderr,
            child_stdin,
            child_stdout,
            child_stderr,
        } = self;
        drop(child_stdin);
        drop(child_stdout);
        drop(child_stderr);
        (parent_stdin, parent_stdout, parent_stderr)
    }
}

fn pipe(parent_reads: bool) -> Result<(NativeHandle, File), LaunchError> {
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(|_| LaunchError::Io)?,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    // SAFETY: output pointers and security structure are valid.
    if unsafe { CreatePipe(&mut read, &mut write, &security, 65_536) } == 0 {
        return Err(LaunchError::Io);
    }
    let read = NativeHandle::new(read)?;
    let write = NativeHandle::new(write)?;
    let (child, parent) = if parent_reads {
        (write, read)
    } else {
        (read, write)
    };
    // SAFETY: parent is live; clearing inherit prevents ambient inheritance.
    if unsafe { SetHandleInformation(parent.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(LaunchError::ContainmentFailure);
    }
    let raw = parent.raw();
    std::mem::forget(parent);
    // SAFETY: ownership of raw transfers exactly once to File.
    let file = unsafe { File::from_raw_handle(raw as RawHandle) };
    Ok((child, file))
}
