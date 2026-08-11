use std::path::{Path, PathBuf};

use super::{AuthenticatedHelperRequest, ElevatedHelperError, constant_time_equal};
use serde::{Deserialize, Serialize};

const JOURNAL_SCHEMA_VERSION: u8 = 1;
const JOURNAL_BYTES_MAX: usize = 64 * 1024;
const RECOVERY_STEPS_MAX: usize = 4;
const JOURNAL_NAME: &str = ".sage-codex-0.145.0-provisioning-journal.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum JournalPhase {
    Prepared,
    StatePublished,
    PackagePublished,
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
    fn revalidate_stage(&mut self) -> Result<(), ElevatedHelperError>;
    fn revalidate_stage_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn revalidate_state(&mut self) -> Result<(), ElevatedHelperError>;
    fn apply_package_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn create_state_with_acl(&mut self) -> Result<(), ElevatedHelperError>;
    fn publish_package_write_through(&mut self) -> Result<(), ElevatedHelperError>;
    fn final_managed_proof(&mut self) -> Result<Self::Proof, ElevatedHelperError>;
    fn remove_completed_stage(&mut self) -> Result<(), ElevatedHelperError>;
    fn rollback_package(&mut self) -> Result<(), ElevatedHelperError>;
    fn rollback_state(&mut self) -> Result<(), ElevatedHelperError>;
}

pub(super) fn finalize<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
) -> Result<P::Proof, ElevatedHelperError> {
    let journal_path = journal_path(request)?;
    let expected = initial_journal(request)?;
    let mut journal = match platform.read_journal(&journal_path, JOURNAL_BYTES_MAX)? {
        Some(bytes) => decode_journal(&bytes, &expected, request)?,
        None => {
            require_namespace(platform.observe()?, JournalPhase::Prepared, false)?;
            platform.revalidate_stage()?;
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
                    platform.create_state_with_acl()?;
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
                    platform.publish_package_write_through()?;
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
    if observed.final_state {
        platform
            .rollback_state()
            .map_err(|_| ElevatedHelperError::Rollback)?;
    }
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
) -> Result<SealedJournal, ElevatedHelperError> {
    let payload = JournalPayload {
        schema_version: JOURNAL_SCHEMA_VERSION,
        creation_nonce: request.creation_nonce().to_owned(),
        request_binding_sha256: request.binding_sha256()?,
        stage_root: request.stage_root().to_owned(),
        package_target: request.package_target().to_owned(),
        state_target: request.state_target().to_owned(),
        phase: JournalPhase::Prepared,
    };
    Ok(SealedJournal {
        authenticator_sha256: seal_payload(request, &payload)?,
        payload,
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
    expected: &SealedJournal,
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
    let mut expected_payload = expected.payload.clone();
    expected_payload.phase = journal.payload.phase;
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
