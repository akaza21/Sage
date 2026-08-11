use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    CreateWellKnownSid, DuplicateToken, GetLengthSid, GetTokenInformation, IsValidSid, PSID,
    SECURITY_MAX_SID_SIZE, SecurityImpersonation, TOKEN_DUPLICATE, TOKEN_QUERY, TOKEN_USER,
    TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::{ManagedStagingError, OwnerClass};

const TOKEN_BYTES_MAX: usize = 64 * 1024;

#[derive(Debug)]
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: this value is created only from successful Win32 handle calls.
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) struct ProcessToken {
    _primary: OwnedHandle,
    impersonation: OwnedHandle,
    pub(super) user_sid: Vec<u8>,
    administrators_sid: Vec<u8>,
    system_sid: Vec<u8>,
}

impl ProcessToken {
    pub(super) fn open() -> Result<Self, ManagedStagingError> {
        let mut primary = std::ptr::null_mut();
        // SAFETY: output handle is writable and GetCurrentProcess is always valid.
        if unsafe {
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_QUERY | TOKEN_DUPLICATE,
                &mut primary,
            )
        } == 0
        {
            return Err(ManagedStagingError::Io);
        }
        let primary = OwnedHandle(primary);
        let mut impersonation = std::ptr::null_mut();
        // SAFETY: primary is live and output handle is writable.
        if unsafe { DuplicateToken(primary.0, SecurityImpersonation, &mut impersonation) } == 0 {
            return Err(ManagedStagingError::Io);
        }
        let impersonation = OwnedHandle(impersonation);
        Ok(Self {
            user_sid: token_user_sid(primary.0)?,
            administrators_sid: well_known_sid(WinBuiltinAdministratorsSid)?,
            system_sid: well_known_sid(WinLocalSystemSid)?,
            _primary: primary,
            impersonation,
        })
    }

    pub(super) fn impersonation_handle(&self) -> HANDLE {
        self.impersonation.0
    }

    pub(super) fn classify_owner(&self, sid: &[u8]) -> OwnerClass {
        if sid == self.system_sid {
            OwnerClass::LocalSystem
        } else if sid == self.administrators_sid {
            OwnerClass::BuiltinAdministrators
        } else if sid == self.user_sid {
            OwnerClass::CurrentUser
        } else {
            OwnerClass::Other
        }
    }

    pub(super) fn trusted_mutation_principal(&self, sid: &[u8], state: bool) -> bool {
        sid == self.system_sid || sid == self.administrators_sid || (state && sid == self.user_sid)
    }
}

fn token_user_sid(token: HANDLE) -> Result<Vec<u8>, ManagedStagingError> {
    let mut needed = 0_u32;
    // SAFETY: sizing call supplies no output buffer.
    unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
    let needed = usize::try_from(needed).map_err(|_| ManagedStagingError::Io)?;
    if needed == 0 || needed > TOKEN_BYTES_MAX {
        return Err(ManagedStagingError::Io);
    }
    let mut words = vec![0_u64; needed.div_ceil(std::mem::size_of::<u64>())];
    let mut returned = 0_u32;
    // SAFETY: words is aligned, writable, and large enough.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            words.as_mut_ptr().cast(),
            u32::try_from(needed).map_err(|_| ManagedStagingError::Io)?,
            &mut returned,
        )
    } == 0
    {
        return Err(ManagedStagingError::Io);
    }
    // SAFETY: the TOKEN_USER buffer is aligned and live for the copy.
    let sid = unsafe { (*(words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
    copy_valid_sid(sid)
}

fn well_known_sid(kind: i32) -> Result<Vec<u8>, ManagedStagingError> {
    let mut sid = vec![0_u8; SECURITY_MAX_SID_SIZE as usize];
    let mut bytes = SECURITY_MAX_SID_SIZE;
    // SAFETY: sid is writable for bytes and no domain SID is required.
    if unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            sid.as_mut_ptr().cast(),
            &mut bytes,
        )
    } == 0
    {
        return Err(ManagedStagingError::Io);
    }
    sid.truncate(bytes as usize);
    Ok(sid)
}

fn copy_valid_sid(sid: PSID) -> Result<Vec<u8>, ManagedStagingError> {
    // SAFETY: caller obtained SID from a successful Win32 security query.
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(ManagedStagingError::Io);
    }
    // SAFETY: sid is valid.
    let bytes = unsafe { GetLengthSid(sid) } as usize;
    if bytes == 0 || bytes > SECURITY_MAX_SID_SIZE as usize {
        return Err(ManagedStagingError::Io);
    }
    // SAFETY: a valid SID is readable for GetLengthSid bytes.
    Ok(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), bytes) }.to_vec())
}
