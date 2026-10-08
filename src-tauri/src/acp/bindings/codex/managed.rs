use std::path::PathBuf;

use super::{
    manifest::{MetadataValidatedPacket, Platform},
    physical::{ConfiguredManagedPackageRoot, PhysicallyInspectedPackage, inspect_managed_package},
    reproduction::ReproducedPackagePrerequisite,
};

#[cfg(windows)]
mod windows;

const MANAGED_NODE_MAX: usize = 64;
const SECURITY_DESCRIPTOR_BYTES_MAX: usize = 64 * 1024;
const SECURITY_DESCRIPTOR_AGGREGATE_BYTES_MAX: usize = 1024 * 1024;

const DELETE_ACCESS: u32 = 0x0001_0000;
const WRITE_DAC_ACCESS: u32 = 0x0004_0000;
const WRITE_OWNER_ACCESS: u32 = 0x0008_0000;
const FILE_WRITE_DATA_ACCESS: u32 = 0x0000_0002;
const FILE_APPEND_DATA_ACCESS: u32 = 0x0000_0004;
const FILE_WRITE_EA_ACCESS: u32 = 0x0000_0010;
const FILE_DELETE_CHILD_ACCESS: u32 = 0x0000_0040;
const FILE_WRITE_ATTRIBUTES_ACCESS: u32 = 0x0000_0100;

const DANGEROUS_ACCESS: u32 = DELETE_ACCESS
    | WRITE_DAC_ACCESS
    | WRITE_OWNER_ACCESS
    | FILE_WRITE_DATA_ACCESS
    | FILE_APPEND_DATA_ACCESS
    | FILE_WRITE_EA_ACCESS
    | FILE_DELETE_CHILD_ACCESS
    | FILE_WRITE_ATTRIBUTES_ACCESS;
const STATE_REQUIRED_ACCESS: u32 = FILE_WRITE_DATA_ACCESS
    | FILE_APPEND_DATA_ACCESS
    | FILE_WRITE_EA_ACCESS
    | FILE_DELETE_CHILD_ACCESS
    | FILE_WRITE_ATTRIBUTES_ACCESS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerClass {
    LocalSystem,
    BuiltinAdministrators,
    CurrentUser,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeSecurityClaims {
    path: PathBuf,
    directory: bool,
    owner: OwnerClass,
    dacl_present: bool,
    dacl_protected: bool,
    descriptor_digest: [u8; 32],
    descriptor_bytes: usize,
    effective_access: u32,
    foreign_mutation_grant: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManagedSecuritySnapshot {
    token_user_digest: [u8; 32],
    package_nodes: Vec<NodeSecurityClaims>,
    state_root: NodeSecurityClaims,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ManagedStagingError {
    #[error("managed Codex staging inspection is unavailable on this platform")]
    PlatformUnavailable,
    #[error("the managed Codex package prerequisite is not bound to this packet")]
    Binding,
    #[error("the managed Codex namespace exceeds its node limit")]
    NodeLimit,
    #[error("a managed Codex security descriptor exceeds its byte limit")]
    DescriptorLimit,
    #[error("a managed Codex DACL exceeds its ACE limit")]
    AceLimit,
    #[error("the managed Codex package owner or DACL is not restricted")]
    PackageAcl,
    #[error("the isolated Codex state root owner or DACL is not restricted")]
    StateAcl,
    #[error("the managed Codex security proof changed after inspection")]
    SecurityDrift,
    #[error("managed Codex staging inspection failed")]
    Io,
}

#[derive(Debug)]
enum ManagedProof {
    #[cfg(windows)]
    Windows(Box<windows::WindowsManagedProof>),
    #[cfg(not(windows))]
    Unavailable,
    #[cfg(test)]
    Fixture,
}

/// A retained, packet-bound observation of the managed package namespace and
/// exact isolated state root. The Windows proof covers canonical paths,
/// per-entry owners/DACLs, effective access for the current process token,
/// conservative foreign mutation grants, and descriptor drift. It does not
/// prove provisioning, machine-wide immutability, signatures, or admission.
#[derive(Debug)]
pub(crate) struct ManagedStagingPrerequisite {
    package_root: PathBuf,
    codex_home: PathBuf,
    artifact_count: usize,
    byte_len: u64,
    proof: ManagedProof,
}

impl ManagedStagingPrerequisite {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
        reproduced: &ReproducedPackagePrerequisite,
    ) -> Result<(), ManagedStagingError> {
        reproduced
            .revalidate(packet)
            .map_err(|_| ManagedStagingError::Binding)?;
        let inspection = reproduced.inspection();
        if !inspection.binds_packet(packet)
            || inspection.package_root() != self.package_root
            || inspection.codex_home() != self.codex_home
            || inspection.artifact_count() != self.artifact_count
            || inspection.byte_len() != self.byte_len
        {
            return Err(ManagedStagingError::Binding);
        }
        match &self.proof {
            #[cfg(windows)]
            ManagedProof::Windows(proof) => windows::revalidate(packet, proof),
            #[cfg(not(windows))]
            ManagedProof::Unavailable => Err(ManagedStagingError::PlatformUnavailable),
            #[cfg(test)]
            ManagedProof::Fixture => Ok(()),
        }
    }

    #[cfg(test)]
    pub(super) fn fixture(
        packet: &MetadataValidatedPacket,
        reproduced: &ReproducedPackagePrerequisite,
    ) -> Self {
        let inspection = reproduced.inspection();
        assert!(inspection.binds_packet(packet));
        Self {
            package_root: inspection.package_root().to_owned(),
            codex_home: inspection.codex_home().to_owned(),
            artifact_count: inspection.artifact_count(),
            byte_len: inspection.byte_len(),
            proof: ManagedProof::Fixture,
        }
    }
}

pub(crate) fn inspect_managed_staging(
    packet: &MetadataValidatedPacket,
    reproduced: &ReproducedPackagePrerequisite,
) -> Result<ManagedStagingPrerequisite, ManagedStagingError> {
    reproduced
        .revalidate(packet)
        .map_err(|_| ManagedStagingError::Binding)?;
    if packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(ManagedStagingError::PlatformUnavailable);
    }
    #[cfg(windows)]
    {
        let reproduced_inspection = reproduced.inspection();
        let root =
            ConfiguredManagedPackageRoot::new(reproduced_inspection.package_root().to_owned())
                .map_err(|_| ManagedStagingError::Binding)?;
        let inspection =
            inspect_managed_package(packet, &root).map_err(|_| ManagedStagingError::Binding)?;
        let proof = windows::inspect(packet, inspection)?;
        Ok(ManagedStagingPrerequisite {
            package_root: reproduced_inspection.package_root().to_owned(),
            codex_home: reproduced_inspection.codex_home().to_owned(),
            artifact_count: reproduced_inspection.artifact_count(),
            byte_len: reproduced_inspection.byte_len(),
            proof: ManagedProof::Windows(Box::new(proof)),
        })
    }
    #[cfg(not(windows))]
    {
        Err(ManagedStagingError::PlatformUnavailable)
    }
}

/// Re-inspects the canonical packet paths after the privileged provisioning
/// transaction. This is intentionally separate from reproduced-package
/// admission: it can close provisioning, but grants no catalog or launch
/// authority without the normal upstream prerequisites.
#[cfg(windows)]
pub(super) fn inspect_provisioned_managed_staging(
    packet: &MetadataValidatedPacket,
) -> Result<ManagedStagingPrerequisite, ManagedStagingError> {
    if packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(ManagedStagingError::PlatformUnavailable);
    }
    let root = ConfiguredManagedPackageRoot::new(packet.managed_package_root())
        .map_err(|_| ManagedStagingError::Binding)?;
    let inspection =
        inspect_managed_package(packet, &root).map_err(|_| ManagedStagingError::Binding)?;
    let package_root = inspection.package_root().to_owned();
    let codex_home = inspection.codex_home().to_owned();
    let artifact_count = inspection.artifact_count();
    let byte_len = inspection.byte_len();
    let proof = windows::inspect(packet, inspection)?;
    Ok(ManagedStagingPrerequisite {
        package_root,
        codex_home,
        artifact_count,
        byte_len,
        proof: ManagedProof::Windows(Box::new(proof)),
    })
}

fn validate_snapshot(snapshot: &ManagedSecuritySnapshot) -> Result<(), ManagedStagingError> {
    if snapshot.package_nodes.is_empty() || snapshot.package_nodes.len() > MANAGED_NODE_MAX {
        return Err(ManagedStagingError::NodeLimit);
    }
    let mut descriptor_bytes = snapshot.state_root.descriptor_bytes;
    for node in &snapshot.package_nodes {
        if node.descriptor_bytes > SECURITY_DESCRIPTOR_BYTES_MAX {
            return Err(ManagedStagingError::DescriptorLimit);
        }
        descriptor_bytes = descriptor_bytes
            .checked_add(node.descriptor_bytes)
            .ok_or(ManagedStagingError::DescriptorLimit)?;
        if !node.dacl_present
            || !matches!(
                node.owner,
                OwnerClass::LocalSystem | OwnerClass::BuiltinAdministrators
            )
            || node.effective_access & DANGEROUS_ACCESS != 0
            || node.foreign_mutation_grant
        {
            return Err(ManagedStagingError::PackageAcl);
        }
    }
    if descriptor_bytes > SECURITY_DESCRIPTOR_AGGREGATE_BYTES_MAX
        || snapshot.state_root.descriptor_bytes > SECURITY_DESCRIPTOR_BYTES_MAX
    {
        return Err(ManagedStagingError::DescriptorLimit);
    }
    let package_root = &snapshot.package_nodes[0];
    if !package_root.directory || !package_root.dacl_protected {
        return Err(ManagedStagingError::PackageAcl);
    }
    let state = &snapshot.state_root;
    if !state.directory
        || state.owner != OwnerClass::CurrentUser
        || !state.dacl_present
        || !state.dacl_protected
        || state.foreign_mutation_grant
        || state.effective_access & STATE_REQUIRED_ACCESS != STATE_REQUIRED_ACCESS
    {
        return Err(ManagedStagingError::StateAcl);
    }
    Ok(())
}

fn revalidate_snapshot(
    expected: &ManagedSecuritySnapshot,
    observed: &ManagedSecuritySnapshot,
) -> Result<(), ManagedStagingError> {
    if expected != observed {
        return Err(ManagedStagingError::SecurityDrift);
    }
    validate_snapshot(observed)
}

#[cfg(windows)]
pub(super) fn current_process_user_sid() -> Result<Vec<u8>, ManagedStagingError> {
    windows::current_process_user_sid()
}

#[cfg(windows)]
pub(super) fn validate_provisioned_state_root(
    path: &std::path::Path,
) -> Result<(), ManagedStagingError> {
    windows::validate_state_root(path)
}

#[cfg(windows)]
pub(super) fn validate_provisioned_package_acl(
    package: &PhysicallyInspectedPackage,
) -> Result<(), ManagedStagingError> {
    windows::validate_package_acl(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package_node(index: usize) -> NodeSecurityClaims {
        NodeSecurityClaims {
            path: PathBuf::from(format!(r"C:\SageHarnesses\Codex\0.146.0\package\{index}")),
            directory: index == 0,
            owner: OwnerClass::BuiltinAdministrators,
            dacl_present: true,
            dacl_protected: index == 0,
            descriptor_digest: [index as u8; 32],
            descriptor_bytes: 128,
            effective_access: 0x0012_0089,
            foreign_mutation_grant: false,
        }
    }

    fn state_root() -> NodeSecurityClaims {
        NodeSecurityClaims {
            path: PathBuf::from(r"C:\SageHarnesses\CodexState\codex-app-server-0.146.0-d3992fec"),
            directory: true,
            owner: OwnerClass::CurrentUser,
            dacl_present: true,
            dacl_protected: true,
            descriptor_digest: [90; 32],
            descriptor_bytes: 128,
            effective_access: 0x001f_01ff,
            foreign_mutation_grant: false,
        }
    }

    fn snapshot(node_count: usize) -> ManagedSecuritySnapshot {
        ManagedSecuritySnapshot {
            token_user_digest: [7; 32],
            package_nodes: (0..node_count).map(package_node).collect(),
            state_root: state_root(),
        }
    }

    #[test]
    fn restricted_package_and_private_writable_state_are_accepted() {
        assert_eq!(validate_snapshot(&snapshot(16)), Ok(()));
    }

    #[test]
    fn package_rejects_user_owner_mutation_access_and_foreign_write_grants() {
        let mut user_owner = snapshot(1);
        user_owner.package_nodes[0].owner = OwnerClass::CurrentUser;
        assert_eq!(
            validate_snapshot(&user_owner),
            Err(ManagedStagingError::PackageAcl)
        );

        let mut effective_write = snapshot(1);
        effective_write.package_nodes[0].effective_access |= FILE_WRITE_DATA_ACCESS;
        assert_eq!(
            validate_snapshot(&effective_write),
            Err(ManagedStagingError::PackageAcl)
        );

        let mut foreign = snapshot(1);
        foreign.package_nodes[0].foreign_mutation_grant = true;
        assert_eq!(
            validate_snapshot(&foreign),
            Err(ManagedStagingError::PackageAcl)
        );
    }

    #[test]
    fn state_requires_exact_user_owned_protected_private_writable_root() {
        for change in [
            |value: &mut NodeSecurityClaims| value.owner = OwnerClass::Other,
            |value: &mut NodeSecurityClaims| value.dacl_protected = false,
            |value: &mut NodeSecurityClaims| value.foreign_mutation_grant = true,
            |value: &mut NodeSecurityClaims| value.effective_access &= !FILE_DELETE_CHILD_ACCESS,
        ] {
            let mut changed = snapshot(1);
            change(&mut changed.state_root);
            assert_eq!(
                validate_snapshot(&changed),
                Err(ManagedStagingError::StateAcl)
            );
        }
    }

    #[test]
    fn node_and_descriptor_bounds_accept_exact_and_reject_plus_one() {
        assert_eq!(validate_snapshot(&snapshot(MANAGED_NODE_MAX)), Ok(()));
        assert_eq!(
            validate_snapshot(&snapshot(MANAGED_NODE_MAX + 1)),
            Err(ManagedStagingError::NodeLimit)
        );

        let mut exact = snapshot(1);
        exact.package_nodes[0].descriptor_bytes = SECURITY_DESCRIPTOR_BYTES_MAX;
        assert_eq!(validate_snapshot(&exact), Ok(()));
        exact.package_nodes[0].descriptor_bytes += 1;
        assert_eq!(
            validate_snapshot(&exact),
            Err(ManagedStagingError::DescriptorLimit)
        );

        let mut aggregate = snapshot(MANAGED_NODE_MAX);
        for node in &mut aggregate.package_nodes {
            node.descriptor_bytes = (SECURITY_DESCRIPTOR_AGGREGATE_BYTES_MAX
                - aggregate.state_root.descriptor_bytes)
                / MANAGED_NODE_MAX;
        }
        assert_eq!(validate_snapshot(&aggregate), Ok(()));
        aggregate.package_nodes[0].descriptor_bytes += 1;
        assert_eq!(
            validate_snapshot(&aggregate),
            Err(ManagedStagingError::DescriptorLimit)
        );
    }

    #[test]
    fn descriptor_owner_effective_access_and_token_drift_are_rejected() {
        let expected = snapshot(2);
        for observed in [
            {
                let mut value = expected.clone();
                value.package_nodes[1].descriptor_digest[0] ^= 1;
                value
            },
            {
                let mut value = expected.clone();
                value.package_nodes[1].owner = OwnerClass::LocalSystem;
                value
            },
            {
                let mut value = expected.clone();
                value.package_nodes[1].effective_access ^= 1;
                value
            },
            {
                let mut value = expected.clone();
                value.token_user_digest[0] ^= 1;
                value
            },
        ] {
            assert_eq!(
                revalidate_snapshot(&expected, &observed),
                Err(ManagedStagingError::SecurityDrift)
            );
        }
    }
}
