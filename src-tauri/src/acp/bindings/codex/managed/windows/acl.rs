use windows_sys::Win32::Security::{GENERIC_MAPPING, GetLengthSid, IsValidSid, MapGenericMask};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};

use super::{super::ManagedStagingError, token::ProcessToken};

const ACL_HEADER_BYTES: usize = 8;
const ACE_HEADER_BYTES: usize = 4;
const STANDARD_ACE_SID_OFFSET: usize = 8;
const SID_HEADER_BYTES: usize = 8;
const SID_SUBAUTHORITY_BYTES: usize = 4;
const SID_REVISION: u8 = 1;
const SID_SUBAUTHORITY_MAX: usize = 15;
const ACE_COUNT_MAX: usize = 256;
const ACL_REVISION: u8 = 2;
const ACL_REVISION_DS: u8 = 4;
const ACCESS_ALLOWED_ACE_KIND: u8 = 0;
const ACCESS_DENIED_ACE_KIND: u8 = 1;

pub(super) fn validate_acl_structure(bytes: &[u8]) -> Result<(), ManagedStagingError> {
    scan_acl(bytes, |_, _| false).map(|_| ())
}

pub(super) fn foreign_mutation_grant(
    bytes: &[u8],
    token: &ProcessToken,
    state: bool,
    dangerous_access: u32,
) -> Result<bool, ManagedStagingError> {
    scan_acl(bytes, |mask, sid| {
        mask & dangerous_access != 0 && !token.trusted_mutation_principal(sid, state)
    })
}

fn scan_acl(
    bytes: &[u8],
    mut untrusted_allow: impl FnMut(u32, &[u8]) -> bool,
) -> Result<bool, ManagedStagingError> {
    if bytes.len() < ACL_HEADER_BYTES
        || !matches!(bytes[0], ACL_REVISION | ACL_REVISION_DS)
        || bytes[1] != 0
        || bytes[6..8] != [0, 0]
    {
        return Err(ManagedStagingError::Io);
    }
    let declared_bytes = usize::from(read_u16(bytes, 2)?);
    if declared_bytes != bytes.len()
        || declared_bytes < ACL_HEADER_BYTES
        || declared_bytes % std::mem::align_of::<u32>() != 0
    {
        return Err(ManagedStagingError::Io);
    }
    let ace_count = usize::from(read_u16(bytes, 4)?);
    if ace_count > ACE_COUNT_MAX {
        return Err(ManagedStagingError::AceLimit);
    }

    let mut offset = ACL_HEADER_BYTES;
    let mut foreign_grant = false;
    for _ in 0..ace_count {
        let header_end = offset
            .checked_add(ACE_HEADER_BYTES)
            .filter(|end| *end <= declared_bytes)
            .ok_or(ManagedStagingError::Io)?;
        let header = &bytes[offset..header_end];
        let kind = header[0];
        let ace_bytes = usize::from(u16::from_le_bytes([header[2], header[3]]));
        if ace_bytes < ACE_HEADER_BYTES || ace_bytes % std::mem::align_of::<u32>() != 0 {
            return Err(ManagedStagingError::Io);
        }
        let end = offset
            .checked_add(ace_bytes)
            .filter(|end| *end <= declared_bytes)
            .ok_or(ManagedStagingError::Io)?;
        let ace = &bytes[offset..end];
        match kind {
            ACCESS_ALLOWED_ACE_KIND | ACCESS_DENIED_ACE_KIND => {
                let (mask, sid) = standard_ace(ace)?;
                if kind == ACCESS_ALLOWED_ACE_KIND && untrusted_allow(mask, sid) {
                    foreign_grant = true;
                }
            }
            _ => return Err(ManagedStagingError::Io),
        }
        offset = end;
    }
    Ok(foreign_grant)
}

fn standard_ace(ace: &[u8]) -> Result<(u32, &[u8]), ManagedStagingError> {
    let minimum = STANDARD_ACE_SID_OFFSET
        .checked_add(SID_HEADER_BYTES)
        .ok_or(ManagedStagingError::Io)?;
    if ace.len() < minimum {
        return Err(ManagedStagingError::Io);
    }
    let sid_storage = &ace[STANDARD_ACE_SID_OFFSET..];
    let sid = validated_sid_prefix(sid_storage)?;
    if sid.len() != sid_storage.len() {
        return Err(ManagedStagingError::Io);
    }

    let mut mask = read_u32(ace, ACE_HEADER_BYTES)?;
    // SAFETY: mask and the fixed mapping are valid for the duration of the call.
    unsafe { MapGenericMask(&mut mask, &generic_mapping()) };
    Ok((mask, sid))
}

pub(super) fn validated_sid_prefix(bytes: &[u8]) -> Result<&[u8], ManagedStagingError> {
    if bytes.len() < SID_HEADER_BYTES
        || !(bytes.as_ptr() as usize).is_multiple_of(std::mem::align_of::<u32>())
        || bytes[0] != SID_REVISION
        || usize::from(bytes[1]) > SID_SUBAUTHORITY_MAX
    {
        return Err(ManagedStagingError::Io);
    }
    let sid_bytes = usize::from(bytes[1])
        .checked_mul(SID_SUBAUTHORITY_BYTES)
        .and_then(|bytes| bytes.checked_add(SID_HEADER_BYTES))
        .ok_or(ManagedStagingError::Io)?;
    let sid = bytes.get(..sid_bytes).ok_or(ManagedStagingError::Io)?;
    // SAFETY: the bounded slice contains the exact SID header-derived extent.
    let sid_pointer = sid.as_ptr().cast_mut().cast();
    if unsafe { IsValidSid(sid_pointer) } == 0
        || usize::try_from(unsafe { GetLengthSid(sid_pointer) })
            .map_err(|_| ManagedStagingError::Io)?
            != sid_bytes
    {
        return Err(ManagedStagingError::Io);
    }
    Ok(sid)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ManagedStagingError> {
    let end = offset.checked_add(2).ok_or(ManagedStagingError::Io)?;
    let value: [u8; 2] = bytes
        .get(offset..end)
        .ok_or(ManagedStagingError::Io)?
        .try_into()
        .map_err(|_| ManagedStagingError::Io)?;
    Ok(u16::from_le_bytes(value))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ManagedStagingError> {
    let end = offset.checked_add(4).ok_or(ManagedStagingError::Io)?;
    let value: [u8; 4] = bytes
        .get(offset..end)
        .ok_or(ManagedStagingError::Io)?
        .try_into()
        .map_err(|_| ManagedStagingError::Io)?;
    Ok(u32::from_le_bytes(value))
}

fn generic_mapping() -> GENERIC_MAPPING {
    GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOW: u8 = 0;
    const DENY: u8 = 1;

    fn system_sid(subauthority_count: u8) -> Vec<u8> {
        let mut sid = vec![1, subauthority_count, 0, 0, 0, 0, 0, 5];
        for index in 0..subauthority_count {
            sid.extend_from_slice(&u32::from(index + 18).to_le_bytes());
        }
        sid
    }

    fn standard_ace(kind: u8, sid: &[u8]) -> Vec<u8> {
        let size = 8_usize.checked_add(sid.len()).unwrap();
        let mut ace = vec![kind, 0];
        ace.extend_from_slice(&u16::try_from(size).unwrap().to_le_bytes());
        ace.extend_from_slice(&0_u32.to_le_bytes());
        ace.extend_from_slice(sid);
        ace
    }

    fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
        let size = 8 + aces.iter().map(Vec::len).sum::<usize>();
        let mut bytes = vec![2, 0];
        bytes.extend_from_slice(&u16::try_from(size).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(aces.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        for ace in aces {
            bytes.extend_from_slice(ace);
        }
        bytes
    }

    #[test]
    fn exact_256_standard_aces_are_accepted_and_257_are_rejected() {
        let ace = standard_ace(DENY, &system_sid(1));
        assert_eq!(
            validate_acl_structure(&acl(&vec![ace.clone(); 256])),
            Ok(())
        );
        assert_eq!(
            validate_acl_structure(&acl(&vec![ace; 257])),
            Err(ManagedStagingError::AceLimit)
        );
    }

    #[test]
    fn short_header_truncated_ace_and_declared_acl_overflow_fail_safely() {
        assert_eq!(
            validate_acl_structure(&[2, 0, 8]),
            Err(ManagedStagingError::Io)
        );

        let missing_ace_header = [2, 0, 8, 0, 1, 0, 0, 0];
        assert_eq!(
            validate_acl_structure(&missing_ace_header),
            Err(ManagedStagingError::Io)
        );

        let mut truncated = vec![2, 0, 16, 0, 1, 0, 0, 0, ALLOW, 0, 20, 0];
        truncated.extend_from_slice(&0_u32.to_le_bytes());
        assert_eq!(
            validate_acl_structure(&truncated),
            Err(ManagedStagingError::Io)
        );

        let short_standard_ace = vec![ALLOW, 0, 12, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        assert_eq!(
            validate_acl_structure(&acl(&[short_standard_ace])),
            Err(ManagedStagingError::Io)
        );

        let declared_overflow = [2, 0, 0xff, 0xff, 0, 0, 0, 0];
        assert_eq!(
            validate_acl_structure(&declared_overflow),
            Err(ManagedStagingError::Io)
        );
    }

    #[test]
    fn oversized_or_truncated_sid_never_reaches_native_sid_validation() {
        let mut oversized_sid = standard_ace(ALLOW, &system_sid(1));
        oversized_sid[9] = 16;
        assert_eq!(
            validate_acl_structure(&acl(&[oversized_sid])),
            Err(ManagedStagingError::Io)
        );

        let mut truncated_sid = standard_ace(ALLOW, &system_sid(1));
        truncated_sid[9] = 2;
        assert_eq!(
            validate_acl_structure(&acl(&[truncated_sid])),
            Err(ManagedStagingError::Io)
        );
    }

    #[test]
    fn unknown_object_and_callback_aces_reject_from_their_bounded_header() {
        for kind in [2_u8, 4, 5, 9, 10, 11, 12, 0xff] {
            let ace = vec![kind, 0, 4, 0];
            assert_eq!(
                validate_acl_structure(&acl(&[ace])),
                Err(ManagedStagingError::Io),
                "ACE kind {kind}"
            );
        }
    }
}
