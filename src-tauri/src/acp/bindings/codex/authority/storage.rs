use std::path::PathBuf;

use super::VerifiedCodexConfig;
use crate::acp::bindings::codex::{
    managed::ManagedStagingPrerequisite,
    manifest::{MetadataValidatedPacket, Platform},
    policy::{
        STATE_ALLOCATED_BYTES_MAX, STATE_CREDENTIAL_FILE_BYTES_MAX, STATE_DEPTH_MAX,
        STATE_DIRECTORIES_MAX, STATE_LOGICAL_BYTES_MAX, STATE_REGULAR_FILES_MAX,
        STATE_SINGLE_FILE_BYTES_MAX, SecurityPolicyPrerequisite,
    },
    reproduction::ReproducedPackagePrerequisite,
};

#[cfg(windows)]
#[path = "storage/windows.rs"]
mod windows;

#[cfg(all(test, windows))]
pub(crate) use windows::{inject_after_state_root_open, inject_before_state_recheck};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum StateAuthorityError {
    #[error("Codex physical state authority is unavailable on this platform")]
    PlatformUnavailable,
    #[error("Codex physical state authority is not bound to its prerequisites")]
    Binding,
    #[error("Codex physical state configuration changed or is not pinned")]
    Config,
    #[error("Codex physical state exceeds its regular-file limit")]
    FileLimit,
    #[error("Codex physical state exceeds its directory limit")]
    DirectoryLimit,
    #[error("Codex physical state exceeds its depth limit")]
    DepthLimit,
    #[error("Codex physical state contains an overlong path")]
    PathLimit,
    #[error("a Codex physical state file exceeds its byte limit")]
    FileBytes,
    #[error("Codex credential state exceeds its byte limit")]
    CredentialBytes,
    #[error("Codex physical state exceeds its aggregate logical-byte limit")]
    LogicalBytes,
    #[error("Codex physical state exceeds its aggregate allocated-byte limit")]
    AllocatedBytes,
    #[error("Codex physical state contains a reparse point, hard link, or path alias")]
    Alias,
    #[error("Codex physical state contains a named alternate data stream")]
    Stream,
    #[error("Codex physical state changed during inspection")]
    ChangedDuringInspection,
    #[error("Codex physical state inspection failed")]
    Io,
}

#[derive(Debug)]
pub(crate) struct CodexAuthorityPrerequisite {
    codex_home: PathBuf,
    policy: SecurityPolicyPrerequisite,
    config: VerifiedCodexConfig,
    managed: ManagedStagingPrerequisite,
    #[cfg(windows)]
    proof: windows::WindowsStateProof,
}

impl CodexAuthorityPrerequisite {
    pub(crate) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
        reproduced: &ReproducedPackagePrerequisite,
    ) -> Result<(), StateAuthorityError> {
        if !cfg!(windows) || packet.metadata().platform != Platform::WindowsX86_64 {
            return Err(StateAuthorityError::PlatformUnavailable);
        }
        self.policy
            .revalidate(packet)
            .map_err(|_| StateAuthorityError::Binding)?;
        self.config
            .revalidate(&self.policy)
            .map_err(|_| StateAuthorityError::Config)?;
        self.managed
            .revalidate(packet, reproduced)
            .map_err(|_| StateAuthorityError::Binding)?;
        if packet.codex_home() != self.codex_home {
            return Err(StateAuthorityError::Binding);
        }
        #[cfg(windows)]
        {
            self.proof.revalidate(packet, &self.policy, &self.config)
        }
        #[cfg(not(windows))]
        {
            Err(StateAuthorityError::PlatformUnavailable)
        }
    }
}

pub(crate) fn inspect_codex_authority(
    packet: &MetadataValidatedPacket,
    policy: SecurityPolicyPrerequisite,
    config: VerifiedCodexConfig,
    reproduced: &ReproducedPackagePrerequisite,
    managed: ManagedStagingPrerequisite,
) -> Result<CodexAuthorityPrerequisite, StateAuthorityError> {
    if !cfg!(windows) || packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(StateAuthorityError::PlatformUnavailable);
    }
    policy
        .revalidate(packet)
        .map_err(|_| StateAuthorityError::Binding)?;
    config
        .revalidate(&policy)
        .map_err(|_| StateAuthorityError::Config)?;
    managed
        .revalidate(packet, reproduced)
        .map_err(|_| StateAuthorityError::Binding)?;
    #[cfg(windows)]
    {
        let proof = windows::inspect(packet, &policy, &config)?;
        Ok(CodexAuthorityPrerequisite {
            codex_home: packet.codex_home().to_owned(),
            policy,
            config,
            managed,
            proof,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (policy, config, reproduced, managed);
        Err(StateAuthorityError::PlatformUnavailable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StorageTotals {
    regular_files: u64,
    directories: u64,
    maximum_depth: u8,
    logical_bytes: u64,
    allocated_bytes: u64,
    maximum_file_bytes: u64,
    credential_file_bytes: u64,
}

fn validate_totals(totals: StorageTotals) -> Result<(), StateAuthorityError> {
    if totals.regular_files > STATE_REGULAR_FILES_MAX {
        return Err(StateAuthorityError::FileLimit);
    }
    if totals.directories > STATE_DIRECTORIES_MAX {
        return Err(StateAuthorityError::DirectoryLimit);
    }
    if totals.maximum_depth > STATE_DEPTH_MAX {
        return Err(StateAuthorityError::DepthLimit);
    }
    if totals.maximum_file_bytes > STATE_SINGLE_FILE_BYTES_MAX {
        return Err(StateAuthorityError::FileBytes);
    }
    if totals.credential_file_bytes > STATE_CREDENTIAL_FILE_BYTES_MAX {
        return Err(StateAuthorityError::CredentialBytes);
    }
    if totals.logical_bytes > STATE_LOGICAL_BYTES_MAX {
        return Err(StateAuthorityError::LogicalBytes);
    }
    if totals.allocated_bytes > STATE_ALLOCATED_BYTES_MAX {
        return Err(StateAuthorityError::AllocatedBytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact() -> StorageTotals {
        StorageTotals {
            regular_files: STATE_REGULAR_FILES_MAX,
            directories: STATE_DIRECTORIES_MAX,
            maximum_depth: STATE_DEPTH_MAX,
            logical_bytes: STATE_LOGICAL_BYTES_MAX,
            allocated_bytes: STATE_ALLOCATED_BYTES_MAX,
            maximum_file_bytes: STATE_SINGLE_FILE_BYTES_MAX,
            credential_file_bytes: STATE_CREDENTIAL_FILE_BYTES_MAX,
        }
    }

    #[test]
    fn storage_stop_limits_accept_exact_and_reject_each_plus_one() {
        assert_eq!(validate_totals(exact()), Ok(()));
        for (changed, error) in [
            (
                StorageTotals {
                    regular_files: STATE_REGULAR_FILES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::FileLimit,
            ),
            (
                StorageTotals {
                    directories: STATE_DIRECTORIES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::DirectoryLimit,
            ),
            (
                StorageTotals {
                    maximum_depth: STATE_DEPTH_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::DepthLimit,
            ),
            (
                StorageTotals {
                    maximum_file_bytes: STATE_SINGLE_FILE_BYTES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::FileBytes,
            ),
            (
                StorageTotals {
                    credential_file_bytes: STATE_CREDENTIAL_FILE_BYTES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::CredentialBytes,
            ),
            (
                StorageTotals {
                    logical_bytes: STATE_LOGICAL_BYTES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::LogicalBytes,
            ),
            (
                StorageTotals {
                    allocated_bytes: STATE_ALLOCATED_BYTES_MAX + 1,
                    ..exact()
                },
                StateAuthorityError::AllocatedBytes,
            ),
        ] {
            assert_eq!(validate_totals(changed), Err(error));
        }
    }
}
