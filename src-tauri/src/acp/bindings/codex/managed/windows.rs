use std::ffi::c_void;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Security::{
    ACL, AccessCheck, DACL_SECURITY_INFORMATION, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
    GetKernelObjectSecurity, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
    GetSecurityDescriptorOwner, OWNER_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ALL_ACCESS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
};

use super::{
    DANGEROUS_ACCESS, ManagedSecuritySnapshot, ManagedStagingError, NodeSecurityClaims, OwnerClass,
    SECURITY_DESCRIPTOR_BYTES_MAX, revalidate_snapshot, validate_snapshot,
};
use crate::acp::bindings::codex::{
    manifest::MetadataValidatedPacket,
    physical::{
        ConfiguredManagedPackageRoot, HeldPhysicalNode, PhysicallyInspectedPackage,
        inspect_managed_package, open_exact_state_root, same_native_path,
    },
};

// Stable Win32 access-mask and ACE type values. Keeping these local avoids
// widening Sage's exact reviewed windows-sys feature surface for constants.
const MAXIMUM_ALLOWED_ACCESS: u32 = 0x0200_0000;

mod acl;
mod token;
use token::ProcessToken;

pub(super) fn current_process_user_sid() -> Result<Vec<u8>, ManagedStagingError> {
    Ok(ProcessToken::open()?.user_sid)
}

pub(super) fn validate_state_root(path: &Path) -> Result<(), ManagedStagingError> {
    let (observed_path, handle) =
        open_exact_state_root(path).map_err(|_| ManagedStagingError::StateAcl)?;
    if !same_native_path(&observed_path, path) {
        return Err(ManagedStagingError::StateAcl);
    }
    let token = ProcessToken::open()?;
    let claims = claims_for_handle(observed_path, true, &handle, &token, true)?;
    if claims.owner != OwnerClass::CurrentUser
        || !claims.dacl_present
        || !claims.dacl_protected
        || claims.foreign_mutation_grant
        || claims.effective_access & super::STATE_REQUIRED_ACCESS != super::STATE_REQUIRED_ACCESS
    {
        return Err(ManagedStagingError::StateAcl);
    }
    Ok(())
}

pub(super) fn validate_package_acl(
    package: &PhysicallyInspectedPackage,
) -> Result<(), ManagedStagingError> {
    let token = ProcessToken::open()?;
    let claims = package
        .held_nodes()
        .iter()
        .map(|node| claims_for_node(node, &token, false))
        .collect::<Result<Vec<_>, _>>()?;
    if claims.is_empty()
        || claims.iter().any(|node| {
            !node.dacl_present
                || !matches!(
                    node.owner,
                    OwnerClass::LocalSystem | OwnerClass::BuiltinAdministrators
                )
                || node.effective_access & DANGEROUS_ACCESS != 0
                || node.foreign_mutation_grant
        })
        || claims
            .iter()
            .find(|node| node.path == package.package_root())
            .is_none_or(|root| !root.directory || !root.dacl_protected)
    {
        return Err(ManagedStagingError::PackageAcl);
    }
    Ok(())
}

#[derive(Debug)]
pub(super) struct WindowsManagedProof {
    package: PhysicallyInspectedPackage,
    state_path: PathBuf,
    state_handle: File,
    snapshot: ManagedSecuritySnapshot,
}

struct SecurityDescriptor {
    words: Vec<u64>,
    byte_len: usize,
}

impl SecurityDescriptor {
    fn pointer(&self) -> *mut c_void {
        self.words.as_ptr().cast_mut().cast()
    }

    fn bytes(&self) -> &[u8] {
        // SAFETY: words owns at least byte_len initialized bytes returned by Win32.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr().cast(), self.byte_len) }
    }

    fn acl_bytes(&self, dacl: *mut ACL) -> Result<&[u8], ManagedStagingError> {
        const ACL_HEADER_BYTES: usize = 8;

        let bytes = self.bytes();
        let base = bytes.as_ptr() as usize;
        let address = dacl as usize;
        if !address.is_multiple_of(std::mem::align_of::<u32>()) {
            return Err(ManagedStagingError::Io);
        }
        let offset = address
            .checked_sub(base)
            .filter(|offset| *offset <= bytes.len())
            .ok_or(ManagedStagingError::Io)?;
        offset
            .checked_add(ACL_HEADER_BYTES)
            .filter(|end| *end <= bytes.len())
            .ok_or(ManagedStagingError::Io)?;
        let acl_bytes = usize::from(u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]));
        if acl_bytes < ACL_HEADER_BYTES {
            return Err(ManagedStagingError::Io);
        }
        let end = offset
            .checked_add(acl_bytes)
            .filter(|end| *end <= bytes.len())
            .ok_or(ManagedStagingError::Io)?;
        Ok(&bytes[offset..end])
    }

    fn sid_bytes(&self, sid: PSID) -> Result<&[u8], ManagedStagingError> {
        let bytes = self.bytes();
        let offset = (sid as usize)
            .checked_sub(bytes.as_ptr() as usize)
            .filter(|offset| *offset <= bytes.len())
            .ok_or(ManagedStagingError::Io)?;
        acl::validated_sid_prefix(bytes.get(offset..).ok_or(ManagedStagingError::Io)?)
    }
}

pub(super) fn inspect(
    packet: &MetadataValidatedPacket,
    package: PhysicallyInspectedPackage,
) -> Result<WindowsManagedProof, ManagedStagingError> {
    let (state_path, state_handle) =
        open_exact_state_root(packet.codex_home()).map_err(|_| ManagedStagingError::StateAcl)?;
    if !same_native_path(&state_path, packet.codex_home()) {
        return Err(ManagedStagingError::StateAcl);
    }
    let snapshot = observe(&package, &state_path, &state_handle)?;
    validate_snapshot(&snapshot)?;
    Ok(WindowsManagedProof {
        package,
        state_path,
        state_handle,
        snapshot,
    })
}

pub(super) fn revalidate(
    packet: &MetadataValidatedPacket,
    proof: &WindowsManagedProof,
) -> Result<(), ManagedStagingError> {
    if !same_native_path(&proof.state_path, packet.codex_home())
        || !same_native_path(proof.package.codex_home(), packet.codex_home())
    {
        return Err(ManagedStagingError::Binding);
    }
    let root = ConfiguredManagedPackageRoot::new(proof.package.package_root().to_owned())
        .map_err(|_| ManagedStagingError::Binding)?;
    let fresh =
        inspect_managed_package(packet, &root).map_err(|_| ManagedStagingError::SecurityDrift)?;
    let observed = observe(&fresh, &proof.state_path, &proof.state_handle)?;
    revalidate_snapshot(&proof.snapshot, &observed)
}

fn observe(
    package: &PhysicallyInspectedPackage,
    state_path: &Path,
    state_handle: &File,
) -> Result<ManagedSecuritySnapshot, ManagedStagingError> {
    let token = ProcessToken::open()?;
    let mut package_nodes = package
        .held_nodes()
        .iter()
        .map(|node| claims_for_node(node, &token, false))
        .collect::<Result<Vec<_>, _>>()?;
    package_nodes.sort_by(|left, right| {
        left.path
            .components()
            .count()
            .cmp(&right.path.components().count())
            .then_with(|| left.path.cmp(&right.path))
    });
    let state_root = claims_for_handle(state_path.to_owned(), true, state_handle, &token, true)?;
    Ok(ManagedSecuritySnapshot {
        token_user_digest: Sha256::digest(&token.user_sid).into(),
        package_nodes,
        state_root,
    })
}

fn claims_for_node(
    node: &HeldPhysicalNode,
    token: &ProcessToken,
    state: bool,
) -> Result<NodeSecurityClaims, ManagedStagingError> {
    claims_for_handle(
        node.path.clone(),
        node.directory,
        &node.handle,
        token,
        state,
    )
}

fn claims_for_handle(
    path: PathBuf,
    directory: bool,
    handle: &File,
    token: &ProcessToken,
    state: bool,
) -> Result<NodeSecurityClaims, ManagedStagingError> {
    let descriptor = query_security_descriptor(handle)?;
    let pointer = descriptor.pointer();
    let mut owner: PSID = std::ptr::null_mut();
    let mut owner_defaulted = 0;
    // SAFETY: descriptor is a live self-relative descriptor and outputs are writable.
    if unsafe { GetSecurityDescriptorOwner(pointer, &mut owner, &mut owner_defaulted) } == 0
        || owner.is_null()
        || owner_defaulted != 0
    {
        return Err(ManagedStagingError::Io);
    }
    let owner = token.classify_owner(descriptor.sid_bytes(owner)?);

    let mut dacl_present = 0;
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut dacl_defaulted = 0;
    // SAFETY: descriptor is live and output pointers are writable.
    if unsafe {
        GetSecurityDescriptorDacl(pointer, &mut dacl_present, &mut dacl, &mut dacl_defaulted)
    } == 0
        || dacl_present == 0
        || dacl.is_null()
        || dacl_defaulted != 0
    {
        return Err(if state {
            ManagedStagingError::StateAcl
        } else {
            ManagedStagingError::PackageAcl
        });
    }
    let dacl = descriptor.acl_bytes(dacl).map_err(|_| {
        if state {
            ManagedStagingError::StateAcl
        } else {
            ManagedStagingError::PackageAcl
        }
    })?;
    acl::validate_acl_structure(dacl).map_err(|_| {
        if state {
            ManagedStagingError::StateAcl
        } else {
            ManagedStagingError::PackageAcl
        }
    })?;
    let mut control = 0_u16;
    let mut revision = 0_u32;
    // SAFETY: descriptor is live and outputs are writable.
    if unsafe { GetSecurityDescriptorControl(pointer, &mut control, &mut revision) } == 0 {
        return Err(ManagedStagingError::Io);
    }
    let effective_access = effective_access(pointer, token.impersonation_handle())?;
    let foreign_mutation_grant = acl::foreign_mutation_grant(dacl, token, state, DANGEROUS_ACCESS)
        .map_err(|_| {
            if state {
                ManagedStagingError::StateAcl
            } else {
                ManagedStagingError::PackageAcl
            }
        })?;
    Ok(NodeSecurityClaims {
        path,
        directory,
        owner,
        dacl_present: true,
        dacl_protected: control & SE_DACL_PROTECTED != 0,
        descriptor_digest: Sha256::digest(descriptor.bytes()).into(),
        descriptor_bytes: descriptor.byte_len,
        effective_access,
        foreign_mutation_grant,
    })
}

fn query_security_descriptor(handle: &File) -> Result<SecurityDescriptor, ManagedStagingError> {
    let raw = handle.as_raw_handle() as HANDLE;
    let requested =
        OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut needed = 0_u32;
    // SAFETY: this sizing call intentionally supplies no output buffer.
    unsafe { GetKernelObjectSecurity(raw, requested, std::ptr::null_mut(), 0, &mut needed) };
    let byte_len = usize::try_from(needed).map_err(|_| ManagedStagingError::DescriptorLimit)?;
    if byte_len == 0 || byte_len > SECURITY_DESCRIPTOR_BYTES_MAX {
        return Err(ManagedStagingError::DescriptorLimit);
    }
    let mut words = vec![0_u64; byte_len.div_ceil(std::mem::size_of::<u64>())];
    // SAFETY: words is aligned, writable, and large enough for needed bytes.
    if unsafe {
        GetKernelObjectSecurity(
            raw,
            requested,
            words.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(ManagedStagingError::Io);
    }
    Ok(SecurityDescriptor { words, byte_len })
}

fn effective_access(descriptor: *mut c_void, token: HANDLE) -> Result<u32, ManagedStagingError> {
    let mapping = generic_mapping();
    let mut privileges = [0_u64; 128];
    let mut privilege_bytes =
        u32::try_from(std::mem::size_of_val(&privileges)).map_err(|_| ManagedStagingError::Io)?;
    let mut granted = 0_u32;
    let mut access = 0;
    // SAFETY: descriptor/token are live; all output buffers are writable and bounded.
    if unsafe {
        AccessCheck(
            descriptor,
            token,
            MAXIMUM_ALLOWED_ACCESS,
            &mapping,
            privileges.as_mut_ptr().cast(),
            &mut privilege_bytes,
            &mut granted,
            &mut access,
        )
    } == 0
        || access == 0
    {
        return Err(ManagedStagingError::Io);
    }
    Ok(granted)
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
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;

    use uuid::Uuid;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    use super::*;

    struct FixtureDirectory(PathBuf);

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn snapshot_with_package_node(
        base: &Path,
        token_user_digest: [u8; 32],
        package_node: NodeSecurityClaims,
    ) -> ManagedSecuritySnapshot {
        ManagedSecuritySnapshot {
            token_user_digest,
            package_nodes: vec![
                NodeSecurityClaims {
                    path: base.to_owned(),
                    directory: true,
                    owner: OwnerClass::BuiltinAdministrators,
                    dacl_present: true,
                    dacl_protected: true,
                    descriptor_digest: [0; 32],
                    descriptor_bytes: 1,
                    effective_access: 0,
                    foreign_mutation_grant: false,
                },
                package_node,
            ],
            state_root: NodeSecurityClaims {
                path: base.to_owned(),
                directory: true,
                owner: OwnerClass::CurrentUser,
                dacl_present: true,
                dacl_protected: true,
                descriptor_digest: [0; 32],
                descriptor_bytes: 1,
                effective_access: super::super::STATE_REQUIRED_ACCESS,
                foreign_mutation_grant: false,
            },
        }
    }

    #[test]
    fn ordinary_mutable_file_is_rejected_for_every_owner_class() {
        let base = std::env::temp_dir().join(format!("sage-managed-acl-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let _cleanup = FixtureDirectory(base.clone());
        let path = base.join("ordinary-file");
        fs::write(&path, b"fixture").unwrap();
        let handle = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&path)
            .unwrap();
        let token = ProcessToken::open().unwrap();
        let claims = claims_for_handle(path.clone(), false, &handle, &token, false).unwrap();

        // Preserve and exercise the exact owner Windows reported. Hosted images
        // may inherit BuiltinAdministrators rather than CurrentUser here.
        assert_eq!(claims.path, path);
        assert!(!claims.directory);
        assert!(claims.dacl_present);
        assert_ne!(claims.effective_access & DANGEROUS_ACCESS, 0);
        let token_user_digest = Sha256::digest(&token.user_sid).into();
        assert_eq!(
            super::super::validate_snapshot(&snapshot_with_package_node(
                &base,
                token_user_digest,
                claims.clone(),
            )),
            Err(ManagedStagingError::PackageAcl)
        );

        for owner in [
            OwnerClass::LocalSystem,
            OwnerClass::BuiltinAdministrators,
            OwnerClass::CurrentUser,
            OwnerClass::Other,
        ] {
            let mut dangerous_claims = claims.clone();
            dangerous_claims.owner = owner;
            dangerous_claims.foreign_mutation_grant = false;
            assert_eq!(
                super::super::validate_snapshot(&snapshot_with_package_node(
                    &base,
                    token_user_digest,
                    dangerous_claims,
                )),
                Err(ManagedStagingError::PackageAcl),
                "dangerous ordinary file was admitted for owner {owner:?}"
            );
        }

        for (owner, expected) in [
            (OwnerClass::LocalSystem, Ok(())),
            (OwnerClass::BuiltinAdministrators, Ok(())),
            (
                OwnerClass::CurrentUser,
                Err(ManagedStagingError::PackageAcl),
            ),
            (OwnerClass::Other, Err(ManagedStagingError::PackageAcl)),
        ] {
            let mut safe_control = claims.clone();
            safe_control.owner = owner;
            safe_control.effective_access &= !DANGEROUS_ACCESS;
            safe_control.foreign_mutation_grant = false;
            assert_eq!(
                super::super::validate_snapshot(&snapshot_with_package_node(
                    &base,
                    token_user_digest,
                    safe_control,
                )),
                expected,
                "safe-access owner policy drifted for {owner:?}"
            );
        }
    }
}
