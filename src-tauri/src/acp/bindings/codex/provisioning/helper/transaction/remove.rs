use std::path::Path;

use super::{
    decode_journal, encode_journal, journal_path, journal_payload, persist_phase, seal_payload,
    AuthenticatedHelperRequest, ElevatedHelperError, HelperTransactionPlatform, JournalOperation,
    JournalPhase, NodeIdentity, SealedJournal, JOURNAL_BYTES_MAX,
};

pub(in super::super) fn execute_boundary_remove<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
) -> Result<(), ElevatedHelperError> {
    let path = journal_path(request)?;
    if let Some(bytes) = platform.read_journal(&path, JOURNAL_BYTES_MAX)? {
        let journal = decode_journal(&bytes, request)?;
        if journal.payload.operation != JournalOperation::Remove {
            return Err(ElevatedHelperError::Cleanup);
        }
        return finish_removal(request, platform, &path, journal);
    }
    let observed = platform.observe()?;
    if observed.stage_package || !observed.final_package || !observed.final_state {
        return Err(ElevatedHelperError::Drift);
    }
    let state_identity = platform.bind_state_identity(None)?;
    let journal = initial_removal_journal(request, state_identity)?;
    platform.create_journal(&path, &encode_journal(&journal)?)?;
    finish_removal(request, platform, &path, journal)
}

fn initial_removal_journal(
    request: &AuthenticatedHelperRequest,
    state_identity: NodeIdentity,
) -> Result<SealedJournal, ElevatedHelperError> {
    let payload = journal_payload(
        request,
        state_identity,
        JournalOperation::Remove,
        JournalPhase::Prepared,
    )?;
    Ok(SealedJournal {
        authenticator_sha256: seal_payload(request, &payload)?,
        payload,
    })
}

fn finish_removal<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
    journal_path: &Path,
    mut journal: SealedJournal,
) -> Result<(), ElevatedHelperError> {
    for _ in 0..2 {
        let observed = platform.observe()?;
        match journal.payload.phase {
            JournalPhase::Prepared => {
                if observed.stage_package || !observed.final_state {
                    return Err(ElevatedHelperError::Drift);
                }
                if observed.final_package {
                    platform.retire_published_package()?;
                }
                persist_phase(
                    request,
                    platform,
                    journal_path,
                    &mut journal,
                    JournalPhase::PackageRetired,
                )?;
            }
            JournalPhase::PackageRetired => {
                if observed.stage_package || observed.final_package {
                    return Err(ElevatedHelperError::Drift);
                }
                if observed.final_state {
                    platform.bind_state_identity(Some(journal.payload.state_identity))?;
                    platform.retire_published_state()?;
                }
                let converged = platform.observe()?;
                if converged.stage_package || converged.final_package || converged.final_state {
                    return Err(ElevatedHelperError::Cleanup);
                }
                return platform
                    .remove_journal(journal_path)
                    .map_err(|_| ElevatedHelperError::Cleanup);
            }
            JournalPhase::StatePublished | JournalPhase::PackagePublished => {
                return Err(ElevatedHelperError::Drift);
            }
        }
    }
    Err(ElevatedHelperError::Journal)
}
