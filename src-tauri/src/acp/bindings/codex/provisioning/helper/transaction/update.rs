use super::{AuthenticatedHelperRequest, ElevatedHelperError, HelperTransactionPlatform, finalize};

pub(in super::super) fn execute_boundary_update<P: HelperTransactionPlatform>(
    request: &AuthenticatedHelperRequest,
    platform: &mut P,
) -> Result<P::Proof, ElevatedHelperError> {
    let observed = platform.observe()?;
    if !observed.stage_package || !observed.final_package || !observed.final_state {
        return Err(ElevatedHelperError::Drift);
    }
    platform.hold_published_pair()?;
    match finalize(request, platform) {
        Ok(proof) => platform.discard_held_pair().map(|()| proof),
        Err(ElevatedHelperError::Cleanup) => Err(ElevatedHelperError::Cleanup),
        Err(error) => platform.restore_held_pair().and(Err(error)),
    }
}
