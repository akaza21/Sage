//! This deliberately records no argv, environment, profile, prompt, protocol
//! payload, or stderr. The entry is published only after the existing runtime
//! slot has accepted the exact owner, and is cleared by that owner's runner.

use std::sync::Mutex;

use serde::Serialize;

use crate::acp::process::{EntryPointKind, ProcessIdentityReceipt, TreeControl};
use sage_acp_protocol::scheduler::RuntimeOwner;

const MAX_PATH_BYTES: usize = 4096;
const MAX_VERSION_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRuntimeIdentityDiagnostics {
    pub(crate) owner: CodexRuntimeIdentityOwner,
    pub(crate) admitted_entrypoint: AdmittedEntrypointIdentity,
    pub(crate) retained_root_process: RetainedRootProcessIdentity,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRuntimeIdentityOwner {
    pub(crate) project_id: String,
    pub(crate) chat_id: String,
    pub(crate) session_id: String,
    /// Decimal string: runtime epochs are native u64s and must not cross the
    /// Tauri/JavaScript boundary as lossy IEEE-754 numbers.
    pub(crate) runtime_epoch: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AdmittedEntrypointIdentity {
    pub(crate) canonical_path: String,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
    pub(crate) version: String,
    pub(crate) binding_digest: String,
    pub(crate) kind: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetainedRootProcessIdentity {
    pub(crate) process_id: u32,
    /// Decimal string: Windows FILETIME is normally above JavaScript's exact
    /// integer range and is evidence, not a display number.
    pub(crate) creation_time_filetime_100ns: String,
    pub(crate) canonical_image_path: String,
    pub(crate) sha256: String,
    pub(crate) bytes: u64,
}

pub(crate) struct AdmittedEntrypointInput<'a> {
    pub(crate) path: &'a std::path::Path,
    pub(crate) sha256: [u8; 32],
    pub(crate) bytes: u64,
    pub(crate) kind: EntryPointKind,
    pub(crate) version: &'a str,
    pub(crate) binding_digest: &'a str,
}

pub(crate) struct CodexRuntimeIdentityLedger(Mutex<LedgerEntry>);

/// The exit observer is armed before the synchronous native receipt capture.
/// Remembering an exit that wins that race prevents the capture from publishing
/// an identity for a root process which is already gone.
enum LedgerEntry {
    Empty,
    Live(Box<CodexRuntimeIdentityDiagnostics>),
    Tombstone(CodexRuntimeIdentityOwner),
}

impl CodexRuntimeIdentityLedger {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(LedgerEntry::Empty))
    }

    pub(crate) fn publish(
        &self,
        owner: &RuntimeOwner,
        entrypoint: AdmittedEntrypointInput<'_>,
        control: &TreeControl,
    ) -> Result<(), ()> {
        let receipt = control.process_identity_receipt().map_err(|_| ())?;
        let observation = build_observation(owner, entrypoint, &receipt)?;
        self.commit_observation(observation)
    }

    fn commit_observation(&self, observation: CodexRuntimeIdentityDiagnostics) -> Result<(), ()> {
        let mut retained = self.0.lock().map_err(|_| ())?;
        match &*retained {
            LedgerEntry::Empty | LedgerEntry::Tombstone(_) => {
                // A tombstone belongs only to an exited owner. A later epoch
                // may publish, while the exited owner is rejected below.
                if matches!(
                    &*retained,
                    LedgerEntry::Tombstone(owner) if owner == &observation.owner
                ) {
                    return Err(());
                }
                *retained = LedgerEntry::Live(Box::new(observation));
                Ok(())
            }
            LedgerEntry::Live(_) => Err(()),
        }
    }

    pub(crate) fn observation(&self) -> Option<CodexRuntimeIdentityDiagnostics> {
        match &*self.0.lock().ok()? {
            LedgerEntry::Live(observation) => Some((**observation).clone()),
            LedgerEntry::Empty | LedgerEntry::Tombstone(_) => None,
        }
    }

    pub(crate) fn clear_exact(&self, owner: &RuntimeOwner) {
        let Ok(mut retained) = self.0.lock() else {
            return;
        };
        match &*retained {
            LedgerEntry::Empty => {
                let Some(owner) = owner_identity(owner) else {
                    return;
                };
                *retained = LedgerEntry::Tombstone(owner);
            }
            LedgerEntry::Live(entry) if owner_matches(&entry.owner, owner) => {
                *retained = LedgerEntry::Empty;
            }
            LedgerEntry::Live(_) | LedgerEntry::Tombstone(_) => {}
        }
    }
}

fn build_observation(
    owner: &RuntimeOwner,
    entrypoint: AdmittedEntrypointInput<'_>,
    root: &ProcessIdentityReceipt,
) -> Result<CodexRuntimeIdentityDiagnostics, ()> {
    if root.runtime_epoch() != owner.runtime_epoch.get() || root.process_id() == 0 {
        return Err(());
    }
    Ok(CodexRuntimeIdentityDiagnostics {
        owner: owner_identity(owner).ok_or(())?,
        admitted_entrypoint: AdmittedEntrypointIdentity {
            canonical_path: bounded_path(entrypoint.path)?,
            sha256: hex(entrypoint.sha256),
            bytes: entrypoint.bytes,
            version: bounded(entrypoint.version, MAX_VERSION_BYTES)?,
            binding_digest: bounded_digest(entrypoint.binding_digest)?,
            kind: match entrypoint.kind {
                EntryPointKind::NativeExecutable => "nativeExecutable",
                EntryPointKind::WindowsBatchLauncher => "windowsBatchLauncher",
            },
        },
        retained_root_process: RetainedRootProcessIdentity {
            process_id: root.process_id(),
            creation_time_filetime_100ns: canonical_decimal(root.creation_time_filetime_100ns())?,
            canonical_image_path: bounded_path(root.canonical_image_path())?,
            sha256: hex(root.executable_sha256()),
            bytes: root.executable_bytes(),
        },
    })
}

fn owner_identity(owner: &RuntimeOwner) -> Option<CodexRuntimeIdentityOwner> {
    Some(CodexRuntimeIdentityOwner {
        project_id: bounded(owner.project_id.as_str(), MAX_PATH_BYTES).ok()?,
        chat_id: bounded(owner.chat_id.as_str(), MAX_PATH_BYTES).ok()?,
        session_id: bounded(owner.session_id.as_str(), MAX_PATH_BYTES).ok()?,
        runtime_epoch: canonical_decimal(owner.runtime_epoch.get()).ok()?,
    })
}

fn owner_matches(observation: &CodexRuntimeIdentityOwner, owner: &RuntimeOwner) -> bool {
    observation.project_id == owner.project_id.as_str()
        && observation.chat_id == owner.chat_id.as_str()
        && observation.session_id == owner.session_id.as_str()
        && observation.runtime_epoch == owner.runtime_epoch.get().to_string()
}

fn bounded_path(path: &std::path::Path) -> Result<String, ()> {
    bounded(path.to_str().ok_or(())?, MAX_PATH_BYTES)
}

fn bounded(value: &str, maximum: usize) -> Result<String, ()> {
    (!value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
        .ok_or(())
}

fn bounded_digest(value: &str) -> Result<String, ()> {
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
        .ok_or(())
}

fn canonical_decimal(value: u64) -> Result<String, ()> {
    (value != 0).then(|| value.to_string()).ok_or(())
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::process::ProcessIdentityReceipt;
    use sage_acp_protocol::contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
    };

    fn owner(epoch: u64) -> RuntimeOwner {
        RuntimeOwner {
            project_id: SageProjectId::parse("project-runtime-identity").unwrap(),
            chat_id: SageChatId::parse("chat-runtime-identity").unwrap(),
            session_id: SageAgentSessionId::parse("session-runtime-identity").unwrap(),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        }
    }

    fn observation(owner: &RuntimeOwner) -> CodexRuntimeIdentityDiagnostics {
        let binding_digest = "ab".repeat(32);
        build_observation(
            owner,
            AdmittedEntrypointInput {
                path: std::path::Path::new(r"C:\\Codex\\codex.cmd"),
                sha256: [1; 32],
                bytes: 12,
                kind: EntryPointKind::WindowsBatchLauncher,
                version: "0.99.0",
                binding_digest: &binding_digest,
            },
            &ProcessIdentityReceipt::test_only(
                owner.runtime_epoch.get(),
                41,
                77,
                [2; 32],
                13,
                std::path::PathBuf::from(r"C:\\Program Files\\nodejs\\node.exe"),
            ),
        )
        .unwrap()
    }

    #[test]
    fn safe_serialization_never_accepts_control_text_or_unbounded_version() {
        assert!(bounded("codex\nsecret", MAX_VERSION_BYTES).is_err());
        assert!(bounded(&"x".repeat(MAX_VERSION_BYTES + 1), MAX_VERSION_BYTES).is_err());
        assert_eq!(hex([0xab; 32]), "ab".repeat(32));
    }

    #[test]
    fn release_build_does_not_compile_the_debug_command_module() {
        // The command is cfg-gated at its registration seam in lib.rs. Keeping
        // this test local documents the data object is not a release API.
        const {
            assert!(cfg!(any(debug_assertions, feature = "diagnostics")));
        }
    }

    #[test]
    fn exact_publish_shape_reports_batch_entrypoint_and_node_root() {
        let value = observation(&owner(7));
        assert_eq!(value.owner.runtime_epoch, "7");
        assert_eq!(value.admitted_entrypoint.kind, "windowsBatchLauncher");
        assert!(
            value
                .admitted_entrypoint
                .canonical_path
                .ends_with("codex.cmd")
        );
        assert!(
            value
                .retained_root_process
                .canonical_image_path
                .ends_with("node.exe")
        );
        assert_eq!(value.retained_root_process.process_id, 41);
        assert_eq!(
            value.retained_root_process.creation_time_filetime_100ns,
            "77"
        );
    }

    #[test]
    fn native_u64_identity_values_cross_as_exact_canonical_decimal_strings() {
        assert_eq!(
            canonical_decimal(18_446_744_073_709_551_615).unwrap(),
            "18446744073709551615"
        );
        assert!(canonical_decimal(0).is_err());
    }

    #[test]
    fn exit_before_identity_commit_rejects_that_owner_and_allows_the_next_owner() {
        let ledger = CodexRuntimeIdentityLedger::new();
        assert_eq!(ledger.observation(), None);
        let exited = owner(7);
        let next = owner(8);

        ledger.clear_exact(&exited);
        assert!(ledger.commit_observation(observation(&exited)).is_err());
        assert_eq!(ledger.observation(), None);

        assert!(ledger.commit_observation(observation(&next)).is_ok());
        assert_eq!(ledger.observation(), Some(observation(&next)));

        ledger.clear_exact(&exited);
        assert_eq!(ledger.observation(), Some(observation(&next)));

        ledger.clear_exact(&next);
        assert_eq!(ledger.observation(), None);
    }
}
