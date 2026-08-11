use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::Authorization::{SE_FILE_OBJECT, SetSecurityInfo};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, ACL_REVISION, AddAccessAllowedAceEx, CreateWellKnownSid,
    DACL_SECURITY_INFORMATION, InitializeAcl, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSID, SECURITY_MAX_SID_SIZE,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, GetFileInformationByHandle, READ_CONTROL,
    WRITE_DAC, WRITE_OWNER,
};

use super::super::ElevatedHelperError;
use crate::acp::bindings::codex::physical::HeldPhysicalNode;
use crate::acp::bindings::codex::provisioning::AclPlan;

pub(super) struct Principals {
    pub(super) user: Vec<u8>,
    pub(super) administrators: Vec<u8>,
    pub(super) system: Vec<u8>,
}

pub(super) fn apply_acl_to_exact_node(
    held: &HeldPhysicalNode,
    plan: &AclPlan,
    owner: &[u8],
    principals: &Principals,
) -> Result<(), ElevatedHelperError> {
    let acl_handle = open_acl_node(&held.path, held.directory)?;
    if file_identity(&acl_handle)? != file_identity(&held.handle)? {
        return Err(ElevatedHelperError::Drift);
    }
    apply_acl(&acl_handle, plan, owner, principals, held.directory)?;
    if file_identity(&acl_handle)? != file_identity(&held.handle)? {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(())
}

pub(super) fn open_acl_node(path: &Path, directory: bool) -> Result<File, ElevatedHelperError> {
    let mut options = OpenOptions::new();
    options
        .access_mode(READ_CONTROL | WRITE_DAC | WRITE_OWNER)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        );
    options
        .open(path)
        .map_err(|_| ElevatedHelperError::AclApplication)
}

pub(super) fn apply_acl(
    node: &File,
    plan: &AclPlan,
    owner: &[u8],
    principals: &Principals,
    directory: bool,
) -> Result<(), ElevatedHelperError> {
    let entries = [
        (&principals.system, plan.system_access_mask),
        (&principals.administrators, plan.administrators_access_mask),
        (&principals.user, plan.current_user_access_mask),
    ];
    let byte_len = entries
        .iter()
        .try_fold(8_usize, |total, (sid, _)| {
            total.checked_add(std::mem::size_of::<ACCESS_ALLOWED_ACE>() - 4 + sid.len())
        })
        .ok_or(ElevatedHelperError::AclApplication)?;
    let mut words = vec![0_u32; byte_len.div_ceil(4)];
    let acl = words.as_mut_ptr().cast::<ACL>();
    if unsafe { InitializeAcl(acl, byte_len as u32, ACL_REVISION) } == 0 {
        return Err(ElevatedHelperError::AclApplication);
    }
    let inheritance = if directory {
        OBJECT_INHERIT_ACE | windows_sys::Win32::Security::CONTAINER_INHERIT_ACE
    } else {
        0
    };
    for (sid, access) in entries {
        if unsafe {
            AddAccessAllowedAceEx(
                acl,
                ACL_REVISION,
                inheritance,
                access,
                sid.as_ptr().cast_mut().cast(),
            )
        } == 0
        {
            return Err(ElevatedHelperError::AclApplication);
        }
    }
    let information = OWNER_SECURITY_INFORMATION
        | DACL_SECURITY_INFORMATION
        | PROTECTED_DACL_SECURITY_INFORMATION;
    if unsafe {
        SetSecurityInfo(
            node.as_raw_handle() as HANDLE,
            SE_FILE_OBJECT,
            information,
            owner.as_ptr().cast_mut().cast(),
            std::ptr::null_mut(),
            acl,
            std::ptr::null(),
        )
    } != 0
    {
        return Err(ElevatedHelperError::AclApplication);
    }
    Ok(())
}

fn file_identity(file: &File) -> Result<(u32, u64), ElevatedHelperError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut info) } == 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Drift);
    }
    Ok((
        info.dwVolumeSerialNumber,
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

pub(super) fn well_known_sid(kind: i32) -> Result<Vec<u8>, ElevatedHelperError> {
    let mut sid = vec![0_u8; SECURITY_MAX_SID_SIZE as usize];
    let mut bytes = SECURITY_MAX_SID_SIZE;
    if unsafe {
        CreateWellKnownSid(
            kind,
            std::ptr::null_mut(),
            sid.as_mut_ptr().cast::<std::ffi::c_void>() as PSID,
            &mut bytes,
        )
    } == 0
    {
        return Err(ElevatedHelperError::HelperIdentity);
    }
    sid.truncate(bytes as usize);
    Ok(sid)
}
