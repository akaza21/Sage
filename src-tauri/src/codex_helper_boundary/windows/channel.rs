use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::time::{Duration, Instant};

use uuid::Uuid;
use windows_sys::Win32::Foundation::{
    ERROR_MORE_DATA, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, GetLastError,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    ACL, ACL_REVISION, AddAccessAllowedAce, InitializeAcl, InitializeSecurityDescriptor,
    SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR, SetSecurityDescriptorDacl,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IMPERSONATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_NOWAIT, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, SetNamedPipeHandleState, WaitNamedPipeW,
};

use crate::codex_helper_boundary::protocol::BoundaryError;

const PIPE_PREFIX: &str = r"\\.\pipe\sage-codex-0146-";
const PIPE_BUFFER_BYTES: u32 = 132 * 1024;
const GENERIC_READ_WRITE: u32 = 0xc000_0000;
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
}

pub(crate) struct PipeConnection(pub(crate) File);

impl AsRawHandle for PipeConnection {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        self.0.as_raw_handle()
    }
}

pub(crate) fn fresh_name() -> String {
    format!("{PIPE_PREFIX}{}", Uuid::new_v4().simple())
}

pub(crate) fn validate_name(name: &str) -> Result<(), BoundaryError> {
    let suffix = name
        .strip_prefix(PIPE_PREFIX)
        .ok_or(BoundaryError::Malformed)?;
    if suffix.len() != 32
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(BoundaryError::Malformed);
    }
    Ok(())
}

pub(crate) fn create_server(name: &str, user_sid: &[u8]) -> Result<PipeConnection, BoundaryError> {
    validate_name(name)?;
    let security = PipeSecurity::new(user_sid)?;
    let wide = wide(name);
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            50,
            &security.attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(BoundaryError::Io);
    }
    Ok(PipeConnection(unsafe {
        File::from_raw_handle(handle.cast())
    }))
}

pub(crate) fn accept(pipe: &PipeConnection, deadline: Instant) -> Result<(), BoundaryError> {
    loop {
        if unsafe { ConnectNamedPipe(pipe.as_raw_handle().cast(), std::ptr::null_mut()) } != 0 {
            return Ok(());
        }
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => return Ok(()),
            ERROR_PIPE_LISTENING | ERROR_NO_DATA if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            ERROR_PIPE_LISTENING | ERROR_NO_DATA => return Err(BoundaryError::Deadline),
            _ => return Err(BoundaryError::Io),
        }
    }
}

pub(crate) fn connect_client(
    name: &str,
    expected_server_pid: u32,
    deadline: Instant,
) -> Result<File, BoundaryError> {
    validate_name(name)?;
    let wide = wide(name);
    loop {
        if Instant::now() >= deadline {
            return Err(BoundaryError::Deadline);
        }
        unsafe { WaitNamedPipeW(wide.as_ptr(), 25) };
        let candidate = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IMPERSONATION)
            .open(Path::new(name));
        if let Ok(file) = candidate {
            let mode = PIPE_READMODE_MESSAGE | PIPE_NOWAIT;
            if unsafe {
                SetNamedPipeHandleState(
                    file.as_raw_handle().cast(),
                    &mode,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0
            {
                return Err(BoundaryError::Io);
            }
            let mut observed = 0_u32;
            if unsafe { GetNamedPipeServerProcessId(file.as_raw_handle().cast(), &mut observed) }
                == 0
                || observed != expected_server_pid
            {
                return Err(BoundaryError::Authority);
            }
            return Ok(file);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn write_message(file: &File, bytes: &[u8]) -> Result<(), BoundaryError> {
    let mut written = 0_u32;
    if bytes.is_empty()
        || bytes.len() > PIPE_BUFFER_BYTES as usize
        || unsafe {
            WriteFile(
                file.as_raw_handle().cast(),
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
        || written as usize != bytes.len()
    {
        return Err(BoundaryError::Io);
    }
    Ok(())
}

pub(crate) fn read_message(
    file: &File,
    maximum: usize,
    deadline: Instant,
) -> Result<Vec<u8>, BoundaryError> {
    let mut buffer = vec![0_u8; maximum.checked_add(1).ok_or(BoundaryError::Bound)?];
    loop {
        let mut read = 0_u32;
        if unsafe {
            ReadFile(
                file.as_raw_handle().cast(),
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } != 0
        {
            if read == 0 || read as usize > maximum {
                return Err(BoundaryError::Bound);
            }
            buffer.truncate(read as usize);
            return Ok(buffer);
        }
        match unsafe { GetLastError() } {
            ERROR_NO_DATA | ERROR_PIPE_LISTENING if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            ERROR_MORE_DATA => return Err(BoundaryError::Bound),
            ERROR_NO_DATA | ERROR_PIPE_LISTENING => return Err(BoundaryError::Deadline),
            _ => return Err(BoundaryError::Io),
        }
    }
}

struct PipeSecurity {
    _descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<u64>,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn new(user_sid: &[u8]) -> Result<Self, BoundaryError> {
        if user_sid.is_empty() || user_sid.len() > 68 {
            return Err(BoundaryError::Authority);
        }
        let acl_bytes = align4(size_of::<ACL>() + 8 + user_sid.len());
        let mut descriptor = Box::new(SECURITY_DESCRIPTOR::default());
        let mut acl = vec![0_u64; acl_bytes.div_ceil(size_of::<u64>())];
        let descriptor_ptr = std::ptr::from_mut(descriptor.as_mut()).cast();
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        if unsafe { InitializeSecurityDescriptor(descriptor_ptr, SECURITY_DESCRIPTOR_REVISION) }
            == 0
            || unsafe { InitializeAcl(acl_ptr, acl_bytes as u32, ACL_REVISION) } == 0
            || unsafe {
                AddAccessAllowedAce(
                    acl_ptr,
                    ACL_REVISION,
                    GENERIC_READ_WRITE,
                    user_sid.as_ptr().cast_mut().cast(),
                )
            } == 0
            || unsafe { SetSecurityDescriptorDacl(descriptor_ptr, 1, acl_ptr, 0) } == 0
        {
            return Err(BoundaryError::Authority);
        }
        Ok(Self {
            attributes: SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor_ptr,
                bInheritHandle: 0,
            },
            _descriptor: descriptor,
            _acl: acl,
        })
    }
}

fn align4(value: usize) -> usize {
    value.saturating_add(3) & !3
}

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM_SID: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

    #[test]
    fn pipe_security_owns_the_complete_native_descriptor_layout() {
        let security = PipeSecurity::new(&SYSTEM_SID).unwrap();

        assert_eq!(
            std::mem::size_of_val(&*security._descriptor),
            size_of::<windows_sys::Win32::Security::SECURITY_DESCRIPTOR>()
        );
        let descriptor_address = security.attributes.lpSecurityDescriptor;
        let moved = security;
        assert_eq!(moved.attributes.lpSecurityDescriptor, descriptor_address);
        assert_eq!(
            std::ptr::from_ref(moved._descriptor.as_ref())
                .cast_mut()
                .cast(),
            descriptor_address
        );
    }

    #[test]
    fn pipe_name_is_fixed_and_bounded() {
        let name = fresh_name();
        assert_eq!(validate_name(&name), Ok(()));
        assert_eq!(
            validate_name(&format!("{name}0")),
            Err(BoundaryError::Malformed)
        );
        assert_eq!(
            validate_name(r"\\.\pipe\foreign"),
            Err(BoundaryError::Malformed)
        );
    }
}
