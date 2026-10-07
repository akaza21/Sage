use super::{
    BrowserMcpError, BrowserMcpServer, BrowserMcpSessionState, BrowserMcpToken, MessagePipe,
    relay_stdio,
};
use crate::acp::browser::BrowserCapability;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_MORE_DATA, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING,
    GetLastError, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::{
    ACL, ACL_REVISION, AddAccessAllowedAce, GetLengthSid, GetTokenInformation, InitializeAcl,
    InitializeSecurityDescriptor, IsValidSid, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    SetSecurityDescriptorDacl, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IMPERSONATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::Pipes::{
    CreateNamedPipeW, GetNamedPipeServerProcessId, PIPE_NOWAIT, PIPE_READMODE_MESSAGE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE, PeekNamedPipe, SetNamedPipeHandleState,
    WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const PIPE_PREFIX: &str = r"\\.\pipe\sage-browser-mcp-";
const PIPE_BYTES: u32 = (2 * 1024 * 1024) + 1;
const GENERIC_READ_WRITE: u32 = 0xc000_0000;
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;
const ENV_PIPE: &str = "SAGE_BROWSER_MCP_PIPE";
const ENV_PID: &str = "SAGE_BROWSER_MCP_HOST_PID";
const ENV_SESSION: &str = "SAGE_BROWSER_MCP_SESSION";
const ENV_TOKEN: &str = "SAGE_BROWSER_MCP_TOKEN";

mod host;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ConnectNamedPipe(pipe: *mut c_void, overlapped: *mut c_void) -> i32;
}

pub struct BrowserMcpPipeHost {
    name: String,
    pipe: Pipe,
}

impl BrowserMcpPipeHost {
    pub fn bind() -> Result<Self, BrowserMcpError> {
        let name = format!("{PIPE_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let security = PipeSecurity::new(&current_user_sid()?)?;
        let wide = wide(&name);
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE
                    | PIPE_READMODE_MESSAGE
                    | PIPE_NOWAIT
                    | PIPE_REJECT_REMOTE_CLIENTS,
                2,
                PIPE_BYTES,
                PIPE_BYTES,
                50,
                &security.attributes,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(BrowserMcpError::Unavailable);
        }
        Ok(Self {
            name,
            pipe: Pipe(unsafe { File::from_raw_handle(handle.cast()) }),
        })
    }

    pub fn child_environment(
        &self,
        session_id: &str,
        token: BrowserMcpToken,
    ) -> [(String, String); 4] {
        [
            (ENV_PIPE.into(), self.name.clone()),
            (ENV_PID.into(), std::process::id().to_string()),
            (ENV_SESSION.into(), session_id.into()),
            (ENV_TOKEN.into(), token.into_env_value()),
        ]
    }

    pub(crate) fn serve<P: BrowserCapability>(
        self,
        server: &mut BrowserMcpServer<P>,
        session_id: &str,
        session_state: impl Fn() -> BrowserMcpSessionState,
    ) -> Result<(), BrowserMcpError> {
        host::serve(self, server, session_id, session_state)
    }
}

pub fn run_stdio_helper_if_requested() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 || args[0] != super::HELPER_ARGUMENT {
        return;
    }
    let result = run_stdio_helper();
    if let Err(error) = &result {
        eprintln!("sage-browser helper exited with code 1: {error:?}");
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

fn run_stdio_helper() -> Result<(), BrowserMcpError> {
    let name = std::env::var(ENV_PIPE).map_err(|_| BrowserMcpError::Unauthorized)?;
    let pid = std::env::var(ENV_PID)
        .map_err(|_| BrowserMcpError::Unauthorized)?
        .parse::<u32>()
        .map_err(|_| BrowserMcpError::Unauthorized)?;
    let session = std::env::var(ENV_SESSION).map_err(|_| BrowserMcpError::Unauthorized)?;
    let token = BrowserMcpToken::from_env_value(
        std::env::var(ENV_TOKEN).map_err(|_| BrowserMcpError::Unauthorized)?,
    )?;
    let mut pipe = connect_client(&name, pid, Instant::now() + Duration::from_secs(10))?;
    let probe = pipe
        .0
        .try_clone()
        .map_err(|_| BrowserMcpError::Unavailable)?;
    std::thread::spawn(move || {
        loop {
            if !pipe_alive(&probe) {
                // This process exists only to relay MCP for the bound session.
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    relay_stdio(
        &mut pipe,
        &mut BufReader::new(std::io::stdin().lock()),
        &mut BufWriter::new(std::io::stdout().lock()),
        &session,
        token,
    )
}

fn pipe_alive(file: &File) -> bool {
    unsafe {
        PeekNamedPipe(
            file.as_raw_handle().cast(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        ) != 0
    }
}

struct Pipe(File);

impl MessagePipe for Pipe {
    fn read(&mut self, maximum: usize) -> Result<Option<Vec<u8>>, BrowserMcpError> {
        let mut bytes = vec![0_u8; maximum.checked_add(1).ok_or(BrowserMcpError::Capacity)?];
        let mut read = 0_u32;
        if unsafe {
            ReadFile(
                self.0.as_raw_handle().cast(),
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                &mut read,
                std::ptr::null_mut(),
            )
        } != 0
        {
            if read == 0 || read as usize > maximum {
                return Err(BrowserMcpError::Capacity);
            }
            bytes.truncate(read as usize);
            return Ok(Some(bytes));
        }
        match unsafe { GetLastError() } {
            ERROR_NO_DATA | ERROR_PIPE_LISTENING => {
                std::thread::sleep(Duration::from_millis(25));
                Ok(None)
            }
            ERROR_MORE_DATA => Err(BrowserMcpError::Capacity),
            _ => Err(BrowserMcpError::Unavailable),
        }
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), BrowserMcpError> {
        if bytes.is_empty() || bytes.len() > PIPE_BYTES as usize {
            return Err(BrowserMcpError::Capacity);
        }
        let mut written = 0_u32;
        if unsafe {
            WriteFile(
                self.0.as_raw_handle().cast(),
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                &mut written,
                std::ptr::null_mut(),
            )
        } == 0
            || written as usize != bytes.len()
        {
            return Err(BrowserMcpError::Unavailable);
        }
        Ok(())
    }
}

fn accept(
    pipe: &Pipe,
    session_state: &impl Fn() -> BrowserMcpSessionState,
) -> Result<(), BrowserMcpError> {
    let pending_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match session_state() {
            BrowserMcpSessionState::Pending if Instant::now() < pending_deadline => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            BrowserMcpSessionState::Pending | BrowserMcpSessionState::Ended => {
                return Err(BrowserMcpError::Unavailable);
            }
            BrowserMcpSessionState::Live => {}
        }
        if unsafe { ConnectNamedPipe(pipe.0.as_raw_handle().cast(), std::ptr::null_mut()) } != 0 {
            return (session_state() == BrowserMcpSessionState::Live)
                .then_some(())
                .ok_or(BrowserMcpError::Unavailable);
        }
        match unsafe { GetLastError() } {
            ERROR_PIPE_CONNECTED => {
                return (session_state() == BrowserMcpSessionState::Live)
                    .then_some(())
                    .ok_or(BrowserMcpError::Unavailable);
            }
            ERROR_PIPE_LISTENING | ERROR_NO_DATA => std::thread::sleep(Duration::from_millis(5)),
            _ => return Err(BrowserMcpError::Unavailable),
        }
    }
}

fn connect_client(
    name: &str,
    expected_pid: u32,
    deadline: Instant,
) -> Result<Pipe, BrowserMcpError> {
    validate_name(name)?;
    let wide = wide(name);
    loop {
        if Instant::now() >= deadline {
            return Err(BrowserMcpError::Unavailable);
        }
        unsafe { WaitNamedPipeW(wide.as_ptr(), 25) };
        let candidate = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(SECURITY_SQOS_PRESENT | SECURITY_IMPERSONATION)
            .open(Path::new(name));
        if let Ok(file) = candidate {
            let mode = PIPE_READMODE_MESSAGE | PIPE_NOWAIT;
            let mut observed = 0_u32;
            if unsafe {
                SetNamedPipeHandleState(
                    file.as_raw_handle().cast(),
                    &mode,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } == 0
                || unsafe {
                    GetNamedPipeServerProcessId(file.as_raw_handle().cast(), &mut observed)
                } == 0
                || observed != expected_pid
            {
                return Err(BrowserMcpError::Unauthorized);
            }
            return Ok(Pipe(file));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn validate_name(name: &str) -> Result<(), BrowserMcpError> {
    let suffix = name
        .strip_prefix(PIPE_PREFIX)
        .ok_or(BrowserMcpError::Unauthorized)?;
    if suffix.len() != 32
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(BrowserMcpError::Unauthorized);
    }
    Ok(())
}

fn current_user_sid() -> Result<Vec<u8>, BrowserMcpError> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(BrowserMcpError::Unauthorized);
    }
    let result = (|| {
        let mut needed = 0_u32;
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed == 0 || needed > 64 * 1024 {
            return Err(BrowserMcpError::Unauthorized);
        }
        let mut words = vec![0_u64; (needed as usize).div_ceil(size_of::<u64>())];
        let mut returned = 0_u32;
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                words.as_mut_ptr().cast(),
                needed,
                &mut returned,
            )
        } == 0
            || returned > needed
        {
            return Err(BrowserMcpError::Unauthorized);
        }
        let sid = unsafe { (*(words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
            return Err(BrowserMcpError::Unauthorized);
        }
        let length = unsafe { GetLengthSid(sid) } as usize;
        if length == 0 || length > 68 {
            return Err(BrowserMcpError::Unauthorized);
        }
        Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec())
    })();
    unsafe { CloseHandle(token) };
    result
}

struct PipeSecurity {
    _descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<u64>,
    attributes: SECURITY_ATTRIBUTES,
}

impl PipeSecurity {
    fn new(user_sid: &[u8]) -> Result<Self, BrowserMcpError> {
        if user_sid.is_empty() || user_sid.len() > 68 {
            return Err(BrowserMcpError::Unauthorized);
        }
        let acl_bytes = (size_of::<ACL>() + 8 + user_sid.len() + 3) & !3;
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
            return Err(BrowserMcpError::Unauthorized);
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

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(test)]
mod tests;
