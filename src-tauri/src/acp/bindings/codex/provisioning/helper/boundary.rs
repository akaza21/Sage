use std::path::PathBuf;

use crate::codex_helper_boundary::{
    ElevatedBoundaryAuthority, HelperIdentity, OneShotSecret, PublicationFailure,
};

use super::{
    AuthenticatedHelperRequest, ElevatedHelperError, HELPER_PRODUCT, HELPER_PROTOCOL_VERSION,
    HelperExecutableIdentity, VerifiedHelperExecutable, authenticate_helper_request_with_secret,
    seal_helper_request_with_secret, windows,
};
use crate::acp::bindings::codex::managed::ManagedStagingPrerequisite;
use crate::acp::bindings::codex::manifest::{MetadataValidatedPacket, parse_packet_metadata};
use crate::acp::bindings::codex::provisioning::{SealedHandoff, hex};

const PINNED_PACKET: &[u8] =
    include_bytes!("../../../../../../testdata/acp/codex/candidate-windows.json");

pub(super) struct AuthenticatedBoundaryPublication {
    process: windows::ElevatedHelperProcess,
    request: AuthenticatedHelperRequest,
    packet: MetadataValidatedPacket,
}

impl AuthenticatedBoundaryPublication {
    #[cfg(test)]
    pub(super) fn creation_nonce(&self) -> &str {
        self.request.creation_nonce()
    }

    fn publish(self) -> Result<ManagedStagingPrerequisite, ElevatedHelperError> {
        windows::execute_authenticated_request(&self.process, &self.request, &self.packet)
    }
}

pub(super) fn seal_boundary_helper_request(
    handoff: &SealedHandoff,
    stage_root: PathBuf,
    helper: HelperIdentity,
    secret: &OneShotSecret,
) -> Result<Vec<u8>, ElevatedHelperError> {
    let helper = observed_helper(helper);
    seal_helper_request_with_secret(handoff, stage_root, &helper, secret.bytes())
}

pub(super) fn authenticate_boundary_publication(
    authority: &ElevatedBoundaryAuthority,
    bytes: &[u8],
    secret: &OneShotSecret,
    helper: HelperIdentity,
) -> Result<AuthenticatedBoundaryPublication, ElevatedHelperError> {
    let packet =
        parse_packet_metadata(PINNED_PACKET).map_err(|_| ElevatedHelperError::Malformed)?;
    let helper = observed_helper(helper);
    let request = authenticate_helper_request_with_secret(bytes, secret.bytes(), &helper, &packet)?;
    Ok(AuthenticatedBoundaryPublication {
        process: windows::ElevatedHelperProcess::from_authenticated_boundary(authority),
        request,
        packet,
    })
}

pub(crate) fn execute_boundary_publication(
    authority: &ElevatedBoundaryAuthority,
    bytes: &[u8],
    secret: &OneShotSecret,
    helper: HelperIdentity,
) -> Result<ManagedStagingPrerequisite, PublicationFailure> {
    authenticate_boundary_publication(authority, bytes, secret, helper)
        .map_err(classify_publication_error)?
        .publish()
        .map_err(classify_publication_error)
}

fn classify_publication_error(error: ElevatedHelperError) -> PublicationFailure {
    if error == ElevatedHelperError::Cleanup {
        PublicationFailure::RecoveryRequired
    } else {
        PublicationFailure::Rejected
    }
}

fn observed_helper(helper: HelperIdentity) -> VerifiedHelperExecutable {
    VerifiedHelperExecutable(HelperExecutableIdentity {
        product: HELPER_PRODUCT.into(),
        protocol_version: HELPER_PROTOCOL_VERSION,
        sage_version: env!("CARGO_PKG_VERSION").into(),
        executable_sha256: hex(&helper.sha256),
        executable_byte_len: helper.byte_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_maps_to_recovery_required_while_other_core_errors_are_rejected() {
        assert_eq!(
            classify_publication_error(ElevatedHelperError::Cleanup),
            PublicationFailure::RecoveryRequired
        );
        assert_eq!(
            classify_publication_error(ElevatedHelperError::Rollback),
            PublicationFailure::Rejected
        );
    }
}
