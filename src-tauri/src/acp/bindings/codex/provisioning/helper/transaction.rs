use std::path::{Path, PathBuf};

use super::{AuthenticatedHelperRequest, ElevatedHelperError, constant_time_equal};
use serde::{Deserialize, Serialize};

#[path = "transaction/remove.rs"]
mod remove;
#[path = "transaction/update.rs"]
mod update;

pub(super) use remove::execute_boundary_remove;
pub(super) use update::execute_boundary_update;

const JOURNAL_SCHEMA_VERSION: u8 = 2;
const JOURNAL_BYTES_MAX: usize = 64 * 1024;
const RECOVERY_STEPS_MAX: usize = 4;
const JOURNAL_NAME: &str = ".sage-codex-0.145.0-provisioning-journal.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum JournalPhase {
    Prepared,
    StatePublished,
    PackagePublished,
    PackageRetired,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum JournalOperation {
    #[default]
    Publish,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalPayload {
    schema_version: u8,
    creation_nonce: String,
    request_binding_sha256: String,
    stage_root: PathBuf,
    package_target: PathBuf,
    state_target: PathBuf,
    state_identity: NodeIdentity,
    #[serde(default)]
    operation: JournalOperation,
    phase: JournalPhase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealedJournal {
    payload: JournalPayload,
    authenticator_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct NamespaceObservation {
    pub(super) stage_package: bool,
    pub(super) final_package: bool,
    pub(super) final_state: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NodeIdentity {
    pub(super) volume_serial: u32,
    pub(super) file_id: u64,
}

pub(super) trait HelperTransactionPlatform {
    type Proof;

    fn read_journal(
        &mut self,
        path: &Path,
        byte_limit: usize,
    ) -> Result<Option<Vec<u8>>, ElevatedHelperError>;
    fn create_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError>;
    fn replace_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError>;
    fn remove_journal(&mut self, path: &Path) -> Result<(), ElevatedHelperError>;
    fn observe(&mut self) -> Result<NamespaceObservation, ElevatedHelperError>;
    fn bind_state_identity(
        &mut self,
        expected: Option<NodeIdentity>,
    ) -> Result<NodeIdentity, ElevatedHelperError>;
    fn revalidate_stage(&mut self) -> Result<(), ElevatedHelperError>;
    fn revalidate_stage_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn revalidate_state(&mut self) -> Result<(), ElevatedHelperError>;
    fn apply_package_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn publish_state_with_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn publish_package_exact(&mut self) -> Result<(), ElevatedHelperError>;
    fn final_managed_proof(&mut self) -> Result<Self::Proof, ElevatedHelperError>;
    fn remove_completed_stage(&mut self) -> Result<(), ElevatedHelperError>;
    fn rollback_package(&mut self) -> Result<(), ElevatedHelperError>;
    fn rollback_state(&mut self) -> Result<(), ElevatedHelperError>;
    fn hold_published_pair(&mut self) -> Result<(), ElevatedHelperError>;
    fn discard_held_pair(&mut self) -> Result<(), ElevatedHelperError>;
    fn restore_held_pair(&mut self) -> Result<(), ElevatedHelperError>;
    fn retire_published_package(&mut self) -> Result<(), ElevatedHelperError>;
    fn retire_published_state(&mut self) -> Result<(), ElevatedHelperError>;
}

pub(super) fn execute_boundary_recover<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
) -> Result<P::Proof, ElevatedHelperError> {
    let path = journal_path(request)?;
    let Some(bytes) = platform.read_journal(&path, JOURNAL_BYTES_MAX)? else {
        return Err(ElevatedHelperError::Drift);
    };
    if decode_journal(&bytes, request)?.payload.operation == JournalOperation::Remove {
        return Err(ElevatedHelperError::Drift);
    }
    finalize(request, platform)
}

pub(super) fn finalize<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
) -> Result<P::Proof, ElevatedHelperError> {
    let journal_path = journal_path(request)?;
    let mut journal = match platform.read_journal(&journal_path, JOURNAL_BYTES_MAX)? {
        Some(bytes) => {
            let journal = decode_journal(&bytes, request)?;
            platform.bind_state_identity(Some(journal.payload.state_identity))?;
            journal
        }
        None => {
            require_namespace(platform.observe()?, JournalPhase::Prepared, false)?;
            platform.revalidate_stage()?;
            let state_identity = platform.bind_state_identity(None)?;
            let expected = initial_journal(request, state_identity)?;
            let bytes = encode_journal(&expected)?;
            platform.create_journal(&journal_path, &bytes)?;
            expected
        }
    };

    let result = advance(request, platform, &journal_path, &mut journal);
    match result {
        Ok(proof) => Ok(proof),
        Err(ElevatedHelperError::Cleanup) => Err(ElevatedHelperError::Cleanup),
        Err(error) => rollback(platform, &journal_path).and(Err(error)),
    }
}

fn advance<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
    journal_path: &Path,
    journal: &mut SealedJournal,
) -> Result<P::Proof, ElevatedHelperError> {
    for _ in 0..RECOVERY_STEPS_MAX {
        let observed = platform.observe()?;
        match journal.payload.phase {
            JournalPhase::Prepared => {
                require_namespace(observed, JournalPhase::Prepared, true)?;
                platform.revalidate_stage()?;
                platform.apply_package_acl()?;
                if observed.final_state {
                    platform.revalidate_state()?;
                } else {
                    platform.publish_state_with_acl()?;
                }
                persist_phase(
                    request,
                    platform,
                    journal_path,
                    journal,
                    JournalPhase::StatePublished,
                )?;
            }
            JournalPhase::StatePublished => {
                require_namespace(observed, JournalPhase::StatePublished, true)?;
                platform.revalidate_state()?;
                if observed.stage_package {
                    platform.revalidate_stage()?;
                    platform.revalidate_stage_acl()?;
                    platform.publish_package_exact()?;
                }
                persist_phase(
                    request,
                    platform,
                    journal_path,
                    journal,
                    JournalPhase::PackagePublished,
                )?;
            }
            JournalPhase::PackagePublished => {
                require_namespace(observed, JournalPhase::PackagePublished, true)?;
                platform.revalidate_state()?;
                let proof = platform.final_managed_proof()?;
                platform
                    .remove_completed_stage()
                    .map_err(|_| ElevatedHelperError::Cleanup)?;
                platform
                    .remove_journal(journal_path)
                    .map_err(|_| ElevatedHelperError::Cleanup)?;
                return Ok(proof);
            }
            JournalPhase::PackageRetired => return Err(ElevatedHelperError::Drift),
        }
    }
    Err(ElevatedHelperError::Journal)
}

fn persist_phase<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
    path: &Path,
    journal: &mut SealedJournal,
    phase: JournalPhase,
) -> Result<(), ElevatedHelperError> {
    journal.payload.phase = phase;
    journal.authenticator_sha256 = seal_payload(request, &journal.payload)?;
    platform.replace_journal(path, &encode_journal(journal)?)
}

fn require_namespace(
    observed: NamespaceObservation,
    phase: JournalPhase,
    journal_exists: bool,
) -> Result<(), ElevatedHelperError> {
    if !journal_exists {
        return if observed.stage_package && !observed.final_package && !observed.final_state {
            Ok(())
        } else if observed.final_package || observed.final_state {
            Err(ElevatedHelperError::Collision)
        } else {
            Err(ElevatedHelperError::Drift)
        };
    }
    let valid = match phase {
        JournalPhase::Prepared => observed.stage_package && !observed.final_package,
        JournalPhase::StatePublished => {
            observed.final_state && (observed.stage_package ^ observed.final_package)
        }
        JournalPhase::PackagePublished => {
            !observed.stage_package && observed.final_package && observed.final_state
        }
        JournalPhase::PackageRetired => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ElevatedHelperError::Drift)
    }
}

fn rollback<P: HelperTransactionPlatform>(
    platform: &mut P,
    journal_path: &Path,
) -> Result<(), ElevatedHelperError> {
    let observed = platform
        .observe()
        .map_err(|_| ElevatedHelperError::Rollback)?;
    if observed.final_package {
        platform
            .rollback_package()
            .map_err(|_| ElevatedHelperError::Rollback)?;
    }
    platform
        .rollback_state()
        .map_err(|_| ElevatedHelperError::Rollback)?;
    let converged = platform
        .observe()
        .map_err(|_| ElevatedHelperError::Rollback)?;
    if !converged.stage_package || converged.final_package || converged.final_state {
        return Err(ElevatedHelperError::Rollback);
    }
    platform
        .remove_journal(journal_path)
        .map_err(|_| ElevatedHelperError::Rollback)
}

fn initial_journal(
    request: &AuthenticatedHelperRequest,
    state_identity: NodeIdentity,
) -> Result<SealedJournal, ElevatedHelperError> {
    let payload = journal_payload(
        request,
        state_identity,
        JournalOperation::Publish,
        JournalPhase::Prepared,
    )?;
    Ok(SealedJournal {
        authenticator_sha256: seal_payload(request, &payload)?,
        payload,
    })
}

fn journal_payload(
    request: &AuthenticatedHelperRequest,
    state_identity: NodeIdentity,
    operation: JournalOperation,
    phase: JournalPhase,
) -> Result<JournalPayload, ElevatedHelperError> {
    Ok(JournalPayload {
        schema_version: JOURNAL_SCHEMA_VERSION,
        creation_nonce: request.creation_nonce().to_owned(),
        request_binding_sha256: request.binding_sha256()?,
        stage_root: request.stage_root().to_owned(),
        package_target: request.package_target().to_owned(),
        state_target: request.state_target().to_owned(),
        state_identity,
        operation,
        phase,
    })
}

fn journal_path(request: &AuthenticatedHelperRequest) -> Result<PathBuf, ElevatedHelperError> {
    request
        .package_target()
        .parent()
        .map(|parent| parent.join(JOURNAL_NAME))
        .ok_or(ElevatedHelperError::HandoffIdentity)
}

fn decode_journal(
    bytes: &[u8],
    request: &AuthenticatedHelperRequest,
) -> Result<SealedJournal, ElevatedHelperError> {
    if bytes.is_empty() || bytes.len() > JOURNAL_BYTES_MAX {
        return Err(ElevatedHelperError::Journal);
    }
    let journal: SealedJournal =
        serde_json::from_slice(bytes).map_err(|_| ElevatedHelperError::Journal)?;
    let expected_authenticator = seal_payload(request, &journal.payload)?;
    if !constant_time_equal(
        journal.authenticator_sha256.as_bytes(),
        expected_authenticator.as_bytes(),
    ) {
        return Err(ElevatedHelperError::Journal);
    }
    let expected_payload = journal_payload(
        request,
        journal.payload.state_identity,
        journal.payload.operation,
        journal.payload.phase,
    )?;
    if journal.payload != expected_payload {
        return Err(ElevatedHelperError::Collision);
    }
    Ok(journal)
}

fn encode_journal(journal: &SealedJournal) -> Result<Vec<u8>, ElevatedHelperError> {
    let bytes = serde_json::to_vec(journal).map_err(|_| ElevatedHelperError::Journal)?;
    if bytes.len() > JOURNAL_BYTES_MAX {
        return Err(ElevatedHelperError::Journal);
    }
    Ok(bytes)
}

fn seal_payload(
    request: &AuthenticatedHelperRequest,
    payload: &JournalPayload,
) -> Result<String, ElevatedHelperError> {
    let bytes = serde_json::to_vec(payload).map_err(|_| ElevatedHelperError::Journal)?;
    Ok(request.journal_authenticator(&bytes))
}

#[cfg(test)]
#[path = "transaction/tests.rs"]
mod tests;
