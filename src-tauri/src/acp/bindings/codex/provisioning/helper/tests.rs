use std::path::PathBuf;

use sha2::Digest;

use super::*;
use crate::acp::bindings::codex::provisioning::{
    AclPlan, HandoffFile, HandoffPayload, SealedHandoff, hex,
};

fn packet() -> crate::acp::bindings::codex::manifest::MetadataValidatedPacket {
    crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap()
}

fn handoff() -> SealedHandoff {
    let packet = packet();
    let payload = HandoffPayload {
        schema_version: 1,
        binding_id: "codex-app-server".into(),
        runtime_version: "0.146.0".into(),
        creation_nonce: "0123456789abcdef0123456789abcdef".into(),
        source_proof_sha256: "11".repeat(32),
        aggregate_byte_len: packet.metadata().package_unpacked_byte_len,
        files: packet
            .physical_artifact_claims()
            .map(|(artifact, relative)| HandoffFile {
                relative_path: PathBuf::from(relative),
                sha256: artifact.sha256.clone(),
                byte_len: artifact.byte_len,
            })
            .collect(),
        package_target: packet.managed_package_root(),
        state_target: packet.codex_home().to_owned(),
        current_user_sid_hex: "010100000000000512000000".into(),
        package_acl: AclPlan {
            owner: "builtinAdministrators".into(),
            protected_dacl: true,
            system_access_mask: 0x001f_01ff,
            administrators_access_mask: 0x001f_01ff,
            current_user_access_mask: 0x0012_00a9,
            inheritance_flags: 3,
        },
        state_acl: AclPlan {
            owner: "currentUser".into(),
            protected_dacl: true,
            system_access_mask: 0x001f_01ff,
            administrators_access_mask: 0x001f_01ff,
            current_user_access_mask: 0x001f_01ff,
            inheritance_flags: 3,
        },
        acl_application_status: "requiredPrivilegedHelperNotApplied".into(),
        publish_status: "notPublished".into(),
    };
    let bytes = serde_json::to_vec(&payload).unwrap();
    SealedHandoff {
        payload,
        payload_sha256: hex(&sha2::Sha256::digest(bytes)),
    }
}

#[test]
fn exact_authenticated_request_still_cannot_cross_unproven_elevation_boundary() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let secret = OneTimeHelperSecret::fixture([0x33; 32]);
    let request = seal_helper_request(
        &handoff(),
        packet
            .managed_package_root()
            .parent()
            .unwrap()
            .join(".sage-codex-0.146.0-stage"),
        &helper,
        &secret,
    )
    .unwrap();

    let authenticated = authenticate_helper_request(&request, &secret, &helper, &packet).unwrap();

    assert_eq!(
        authenticated.creation_nonce(),
        "0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        request_production_elevation(authenticated).unwrap_err(),
        ElevatedHelperError::ElevationBoundaryUnavailable
    );
}

#[test]
fn boundary_secret_and_observed_image_authenticate_the_core_request() {
    use crate::codex_helper_boundary::{
        AuthorityObservation, PublicationAuthority, publication_authority,
    };
    use crate::codex_helper_boundary::{HelperIdentity, OneShotSecret};

    let packet = packet();
    let secret = OneShotSecret::from_bytes([0x33; 32]);
    let helper = HelperIdentity {
        sha256: [0x22; 32],
        byte_len: 1_048_576,
    };
    let request = super::boundary::seal_boundary_helper_request(
        &handoff(),
        packet
            .managed_package_root()
            .parent()
            .unwrap()
            .join(".sage-codex-0.146.0-stage"),
        helper,
        &secret,
    )
    .unwrap();
    let authority = match publication_authority(&AuthorityObservation {
        expected_caller_pid: 41,
        observed_caller_pid: 41,
        expected_caller_sid_sha256: [0x11; 32],
        observed_caller_sid_sha256: [0x11; 32],
        caller_elevated: false,
        helper_elevated: true,
    })
    .unwrap()
    {
        PublicationAuthority::Elevated(authority) => authority,
        PublicationAuthority::ElevationRequired => panic!("expected elevated authority"),
    };

    let authenticated =
        super::boundary::authenticate_boundary_publication(&authority, &request, &secret, helper)
            .unwrap();

    assert_eq!(
        authenticated.creation_nonce(),
        "0123456789abcdef0123456789abcdef"
    );
}

#[test]
fn boundary_sealer_emits_the_requested_lifecycle_operation() {
    use crate::codex_helper_boundary::{HelperIdentity, OneShotSecret};

    let packet = packet();
    let helper = HelperIdentity {
        sha256: [0x22; 32],
        byte_len: 1_048_576,
    };
    let secret = OneShotSecret::from_bytes([0x33; 32]);

    for operation in [
        HelperOperation::Recover,
        HelperOperation::Update,
        HelperOperation::Remove,
    ] {
        let request = super::boundary::seal_boundary_helper_request_operation(
            &handoff(),
            packet
                .managed_package_root()
                .parent()
                .unwrap()
                .join(".sage-codex-0.146.0-stage"),
            helper,
            &secret,
            operation,
        )
        .unwrap();

        assert_eq!(helper_operation(&request), Ok(operation));
    }
}

fn exact_request(helper: &VerifiedHelperExecutable, secret: &OneTimeHelperSecret) -> Vec<u8> {
    let packet = packet();
    seal_helper_request(
        &handoff(),
        packet
            .managed_package_root()
            .parent()
            .unwrap()
            .join(".sage-codex-0.146.0-stage"),
        helper,
        secret,
    )
    .unwrap()
}

#[test]
fn request_byte_limit_accepts_exact_and_rejects_plus_one_and_truncation() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let secret = OneTimeHelperSecret::fixture([0x33; 32]);
    let mut exact = exact_request(&helper, &secret);
    exact.resize(REQUEST_BYTES_MAX, b' ');
    assert!(authenticate_helper_request(&exact, &secret, &helper, &packet).is_ok());
    exact.push(b' ');
    assert_eq!(
        authenticate_helper_request(&exact, &secret, &helper, &packet).unwrap_err(),
        ElevatedHelperError::RequestBytes
    );

    let request = exact_request(&helper, &secret);
    assert_eq!(
        authenticate_helper_request(&request[..request.len() - 1], &secret, &helper, &packet)
            .unwrap_err(),
        ElevatedHelperError::Malformed
    );
}

#[test]
fn secret_mac_and_executable_identity_drift_are_rejected() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let secret = OneTimeHelperSecret::fixture([0x33; 32]);
    let request = exact_request(&helper, &secret);

    assert_eq!(
        authenticate_helper_request(
            &request,
            &OneTimeHelperSecret::fixture([0x34; 32]),
            &helper,
            &packet,
        )
        .unwrap_err(),
        ElevatedHelperError::Authentication
    );
    let mut envelope: HelperRequestEnvelope = serde_json::from_slice(&request).unwrap();
    let replacement = if envelope.authenticator_sha256.starts_with('f') {
        "e"
    } else {
        "f"
    };
    envelope
        .authenticator_sha256
        .replace_range(0..1, replacement);
    assert_eq!(
        authenticate_helper_request(
            &serde_json::to_vec(&envelope).unwrap(),
            &secret,
            &helper,
            &packet,
        )
        .unwrap_err(),
        ElevatedHelperError::Authentication
    );
    for observed in [
        VerifiedHelperExecutable::fixture("0.1.1", [0x22; 32], 1_048_576),
        VerifiedHelperExecutable::fixture("0.1.0", [0x23; 32], 1_048_576),
        VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_577),
    ] {
        assert_eq!(
            authenticate_helper_request(&request, &secret, &observed, &packet).unwrap_err(),
            ElevatedHelperError::HelperIdentity
        );
    }
}

#[test]
fn exact_manifest_source_target_nonce_and_acl_bindings_reject_drift() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let secret = OneTimeHelperSecret::fixture([0x33; 32]);
    let mut cases = Vec::new();

    let mut value = handoff();
    value.payload.creation_nonce.push('0');
    cases.push((value, ElevatedHelperError::HandoffIdentity));
    let mut value = handoff();
    value.payload.source_proof_sha256.replace_range(0..1, "A");
    cases.push((value, ElevatedHelperError::HandoffIdentity));
    let mut value = handoff();
    value.payload.files[0].byte_len += 1;
    cases.push((value, ElevatedHelperError::HandoffIdentity));
    let mut value = handoff();
    value.payload.files.swap(0, 1);
    cases.push((value, ElevatedHelperError::HandoffIdentity));
    let mut value = handoff();
    value.payload.package_target.push("foreign");
    cases.push((value, ElevatedHelperError::HandoffIdentity));
    let mut value = handoff();
    value.payload.state_acl.protected_dacl = false;
    cases.push((value, ElevatedHelperError::AclPlan));

    for (mut changed, expected) in cases {
        changed.payload_sha256 = hex(&sha2::Sha256::digest(
            serde_json::to_vec(&changed.payload).unwrap(),
        ));
        let request = seal_helper_request(
            &changed,
            packet
                .managed_package_root()
                .parent()
                .unwrap()
                .join(".sage-codex-0.146.0-stage"),
            &helper,
            &secret,
        )
        .unwrap();
        assert_eq!(
            authenticate_helper_request(&request, &secret, &helper, &packet).unwrap_err(),
            expected
        );
    }
}

#[test]
fn unsigned_or_observation_only_cannot_update_or_remove() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let secret = OneTimeHelperSecret::fixture([0x33; 32]);
    for operation in [HelperOperation::Update, HelperOperation::Remove] {
        let request = seal_helper_request_operation_with_secret(
            &handoff(),
            packet
                .managed_package_root()
                .parent()
                .unwrap()
                .join(".sage-codex-0.146.0-stage"),
            &helper,
            &secret.0,
            operation,
        )
        .unwrap();
        let authenticated =
            authenticate_helper_request(&request, &secret, &helper, &packet).unwrap();
        assert_eq!(authenticated.operation(), operation);
        assert_eq!(
            request_production_elevation(authenticated).unwrap_err(),
            ElevatedHelperError::ElevationBoundaryUnavailable
        );
    }
}

#[test]
fn journal_authentication_survives_a_fresh_one_shot_secret() {
    let packet = packet();
    let helper = VerifiedHelperExecutable::fixture("0.1.0", [0x22; 32], 1_048_576);
    let first_secret = OneTimeHelperSecret::fixture([0x33; 32]);
    let first_bytes = exact_request(&helper, &first_secret);
    let first = authenticate_helper_request(&first_bytes, &first_secret, &helper, &packet).unwrap();
    let first_authenticator = first.journal_authenticator(b"leftover journal payload");
    drop(first_secret);

    let recovered_secret = OneTimeHelperSecret::fixture([0x44; 32]);
    let recovered_bytes = exact_request(&helper, &recovered_secret);
    let recovered =
        authenticate_helper_request(&recovered_bytes, &recovered_secret, &helper, &packet).unwrap();

    assert_eq!(
        recovered.journal_authenticator(b"leftover journal payload"),
        first_authenticator
    );
}
