use std::{io, slice};

use windows::Win32::{
    Foundation::{ERROR_INVALID_DATA, HLOCAL, LocalFree, NTE_BAD_KEY_STATE},
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};
use windows_core::PCWSTR;
use zeroize::{Zeroize, Zeroizing};

use super::KeyProtector;
use crate::browser::passwords::{VaultError, VaultResult};

/// Extra input DPAPI mixes into the wrap, so a blob from another app's
/// `CryptProtectData` call cannot stand in for Sage's key.
const ENTROPY: &[u8] = b"Sage browser password vault";

/// User-scope DPAPI: only the signed-in Windows account can unwrap the key.
pub(super) struct Dpapi;

fn input_blob(bytes: &[u8]) -> io::Result<CRYPT_INTEGER_BLOB> {
    Ok(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).map_err(io::Error::other)?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

/// Copies a blob DPAPI allocated, wipes the original and frees it.
///
/// # Safety
/// `blob` must come from a successful DPAPI call, which allocates it with
/// `LocalAlloc`.
unsafe fn take_output(blob: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    // SAFETY: DPAPI returned `cbData` initialised bytes at `pbData`.
    let bytes = unsafe { slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize) };
    let copy = Zeroizing::new(bytes.to_vec());
    bytes.zeroize();
    // SAFETY: the buffer came from LocalAlloc and is not used again.
    unsafe { LocalFree(Some(HLOCAL(blob.pbData.cast()))) };
    copy
}

impl KeyProtector for Dpapi {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>> {
        let input = input_blob(key)?;
        let entropy = input_blob(ENTROPY)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: every blob points at live memory for the call, and DPAPI
        // writes only to `output`.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }?;
        // SAFETY: the call succeeded and allocated `output`.
        Ok(unsafe { take_output(output) }.to_vec())
    }

    fn unprotect(&self, wrapped: &[u8]) -> VaultResult<Zeroizing<Vec<u8>>> {
        let input = input_blob(wrapped)?;
        let entropy = input_blob(ENTROPY)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`; no description is requested back.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|error| {
            // A damaged blob, or one another Windows account wrapped.
            if error.code() == ERROR_INVALID_DATA.to_hresult() || error.code() == NTE_BAD_KEY_STATE
            {
                VaultError::KeyLost
            } else {
                VaultError::Io(error.into())
            }
        })?;
        // SAFETY: the call succeeded and allocated `output`.
        Ok(unsafe { take_output(output) })
    }
}
