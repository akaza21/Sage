use std::mem::size_of;
use std::os::windows::io::AsRawHandle;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetLengthSid, GetTokenInformation, IsValidSid, PSID, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
    TokenElevation, TokenUser,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, ImpersonateNamedPipeClient};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

use super::channel::PipeConnection;
use crate::codex_helper_boundary::protocol::BoundaryError;

const TOKEN_BYTES_MAX: usize = 64 * 1024;

pub(crate) struct TokenSubject {
    pub(crate) sid: Vec<u8>,
    pub(crate) sid_sha256: [u8; 32],
    pub(crate) elevated: bool,
}

struct OwnedToken(HANDLE);

impl Drop for OwnedToken {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct ImpersonationGuard<R: FnMut() -> bool> {
    armed: bool,
    revert: R,
}

impl<R: FnMut() -> bool> ImpersonationGuard<R> {
    fn with_revert(revert: R) -> Self {
        Self {
            armed: true,
            revert,
        }
    }

    fn revert(mut self) -> Result<(), BoundaryError> {
        if !(self.revert)() {
            return Err(BoundaryError::Authority);
        }
        self.armed = false;
        Ok(())
    }
}

impl<R: FnMut() -> bool> Drop for ImpersonationGuard<R> {
    fn drop(&mut self) {
        if self.armed {
            let _ = (self.revert)();
        }
    }
}

fn revert_to_self() -> bool {
    unsafe { windows_sys::Win32::Security::RevertToSelf() != 0 }
}

pub(crate) fn current_process_subject() -> Result<TokenSubject, BoundaryError> {
    let mut handle = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) } == 0 {
        return Err(BoundaryError::Authority);
    }
    subject(&OwnedToken(handle))
}

pub(crate) fn connected_client(
    pipe: &PipeConnection,
) -> Result<(u32, TokenSubject), BoundaryError> {
    let mut pid = 0_u32;
    if unsafe { GetNamedPipeClientProcessId(pipe.as_raw_handle().cast(), &mut pid) } == 0
        || pid == 0
    {
        return Err(BoundaryError::Authority);
    }
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle().cast()) } == 0 {
        return Err(BoundaryError::Authority);
    }
    let guard = ImpersonationGuard::with_revert(revert_to_self);
    let mut handle = std::ptr::null_mut();
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut handle) } == 0 {
        return Err(BoundaryError::Authority);
    }
    let observed = subject(&OwnedToken(handle));
    guard.revert()?;
    observed.map(|subject| (pid, subject))
}

fn subject(token: &OwnedToken) -> Result<TokenSubject, BoundaryError> {
    let sid = token_user_sid(token.0)?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0_u32;
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    } == 0
        || returned as usize != size_of::<TOKEN_ELEVATION>()
    {
        return Err(BoundaryError::Authority);
    }
    Ok(TokenSubject {
        sid_sha256: Sha256::digest(&sid).into(),
        sid,
        elevated: elevation.TokenIsElevated != 0,
    })
}

fn token_user_sid(token: HANDLE) -> Result<Vec<u8>, BoundaryError> {
    let mut needed = 0_u32;
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    let needed = needed as usize;
    if needed == 0 || needed > TOKEN_BYTES_MAX {
        return Err(BoundaryError::Authority);
    }
    let mut words = vec![0_u64; needed.div_ceil(size_of::<u64>())];
    let mut returned = 0_u32;
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            words.as_mut_ptr().cast(),
            needed as u32,
            &mut returned,
        )
    } == 0
        || returned as usize > needed
    {
        return Err(BoundaryError::Authority);
    }
    let sid = unsafe { (*(words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    copy_sid(sid)
}

fn copy_sid(sid: PSID) -> Result<Vec<u8>, BoundaryError> {
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(BoundaryError::Authority);
    }
    let bytes = unsafe { GetLengthSid(sid) } as usize;
    if bytes == 0 || bytes > TOKEN_BYTES_MAX {
        return Err(BoundaryError::Authority);
    }
    Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), bytes) }.to_vec())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    #[test]
    fn failed_explicit_revert_keeps_drop_retry_armed() {
        let attempts = Rc::new(Cell::new(0));
        let observed = Rc::clone(&attempts);
        let guard = ImpersonationGuard::with_revert(move || {
            let next = observed.get() + 1;
            observed.set(next);
            next > 1
        });

        assert_eq!(guard.revert(), Err(BoundaryError::Authority));
        assert_eq!(attempts.get(), 2);
    }
}
