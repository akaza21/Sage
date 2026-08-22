use std::path::Path;

use super::super::reproduction::authenticode_unsigned_relative_paths;
use super::{
    AuthenticodeCatalog, INSTALLED_AUTHENTICODE_POLICY, InstalledAuthenticodePolicy, VerifyError,
    WinVerifyTrustCatalog, authenticode_win_verify_trust, bind_packet_artifact,
    verify_installed_authenticode, verify_installed_authenticode_for_observed_digest,
};
use crate::acp::bindings::codex::parse_packet_metadata;

const PACKET_FIXTURE: &[u8] =
    include_bytes!("../../../../../testdata/acp/codex/candidate-windows.json");
const SIGNED_ENTRY: &str = r"vendor\x86_64-pc-windows-msvc\bin\codex.exe";

struct AcceptingCatalog;

impl AuthenticodeCatalog for AcceptingCatalog {
    fn win_verify_trust_file(
        &self,
        _path: &Path,
        policy: InstalledAuthenticodePolicy,
    ) -> Result<(), VerifyError> {
        if policy != INSTALLED_AUTHENTICODE_POLICY {
            return Err(VerifyError::TrustRoot);
        }
        Ok(())
    }
}

fn packet() -> crate::acp::bindings::codex::manifest::MetadataValidatedPacket {
    parse_packet_metadata(PACKET_FIXTURE).unwrap()
}

#[test]
fn ephemeral_public_key_cannot_mint_a_verified_signature() {
    let source = include_str!("../trust_root.rs");
    assert!(
        !source.contains("TrustRootPublicKey"),
        "caller-chosen SPKI types must not exist on this seam"
    );
    assert!(
        !source.contains("spki_der"),
        "caller-chosen public key bytes must not be an input"
    );
    assert!(source.contains("win_verify_trust_file"));
    assert!(source.contains("WINTRUST_ACTION_GENERIC_VERIFY_V2"));
    let err = authenticode_win_verify_trust(Path::new("codex.exe"), INSTALLED_AUTHENTICODE_POLICY);
    assert_eq!(err, Err(VerifyError::Authenticode));
    let production = WinVerifyTrustCatalog
        .win_verify_trust_file(Path::new("codex.exe"), INSTALLED_AUTHENTICODE_POLICY);
    assert_eq!(production, Err(VerifyError::Authenticode));
}

#[test]
fn unbound_signature_is_rejected() {
    let err = bind_packet_artifact(&packet(), "");
    assert_eq!(err, Err(VerifyError::Unbound));
    let err =
        verify_installed_authenticode(&AcceptingCatalog, &packet(), "", Path::new("codex.exe"));
    assert_eq!(err, Err(VerifyError::Unbound));
}

#[test]
fn wrong_artifact_is_rejected() {
    let unsigned = authenticode_unsigned_relative_paths()[0];
    assert_eq!(
        bind_packet_artifact(&packet(), unsigned),
        Err(VerifyError::Artifact)
    );
    assert_eq!(
        verify_installed_authenticode(&AcceptingCatalog, &packet(), unsigned, Path::new("rg.exe"),),
        Err(VerifyError::Artifact)
    );
    assert_eq!(
        verify_installed_authenticode(
            &AcceptingCatalog,
            &packet(),
            "foreign.exe",
            Path::new("foreign.exe"),
        ),
        Err(VerifyError::Artifact)
    );
    let binding = bind_packet_artifact(&packet(), SIGNED_ENTRY).unwrap();
    let ok = verify_installed_authenticode_for_observed_digest(
        &AcceptingCatalog,
        &packet(),
        SIGNED_ENTRY,
        Path::new("codex.exe"),
        binding.artifact_sha256(),
    );
    assert!(ok.is_ok());
    assert_eq!(ok.unwrap().artifact_relative_path(), SIGNED_ENTRY);
}

#[test]
fn signed_pe_cannot_mint_a_signature_for_a_different_claim() {
    let path = std::env::temp_dir().join(format!("sage-wrong-pe-{}.bin", std::process::id()));
    std::fs::write(&path, b"not the claimed artifact bytes").unwrap();
    let err = verify_installed_authenticode(&AcceptingCatalog, &packet(), SIGNED_ENTRY, &path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(err, Err(VerifyError::Artifact));
}
