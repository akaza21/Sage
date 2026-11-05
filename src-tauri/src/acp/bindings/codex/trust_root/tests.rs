use std::path::Path;

#[cfg(windows)]
use std::path::PathBuf;

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

/// A scratch file that removes itself, so attacker bytes never outlive a test.
struct ScratchArtifact(std::path::PathBuf);

impl ScratchArtifact {
    fn with_bytes(bytes: &[u8]) -> Self {
        let path =
            std::env::temp_dir().join(format!("sage-untrusted-{}.exe", uuid::Uuid::new_v4()));
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }
}

impl Drop for ScratchArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn production_catalog_rejects_unsigned_bytes_and_absent_artifacts() {
    let unsigned = ScratchArtifact::with_bytes(b"MZ unsigned bytes from an untrusted publisher");

    assert_eq!(
        authenticode_win_verify_trust(&unsigned.0, INSTALLED_AUTHENTICODE_POLICY),
        Err(VerifyError::Authenticode)
    );
    assert_eq!(
        WinVerifyTrustCatalog.win_verify_trust_file(&unsigned.0, INSTALLED_AUTHENTICODE_POLICY),
        Err(VerifyError::Authenticode)
    );

    let absent = std::env::temp_dir().join(format!("sage-absent-{}.exe", uuid::Uuid::new_v4()));
    assert_eq!(
        authenticode_win_verify_trust(&absent, INSTALLED_AUTHENTICODE_POLICY),
        Err(VerifyError::Artifact)
    );
    assert_eq!(
        WinVerifyTrustCatalog.win_verify_trust_file(&absent, INSTALLED_AUTHENTICODE_POLICY),
        Err(VerifyError::Artifact)
    );
}

#[test]
fn forged_bytes_of_the_pinned_length_cannot_obtain_a_verified_signature() {
    // The smallest signed artifact keeps this fast while still matching the
    // packet's byte-length claim exactly, so only the pinned digest can refuse it.
    let entry = r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-command-runner.exe";
    let binding = bind_packet_artifact(&packet(), entry).unwrap();
    let length = usize::try_from(binding.artifact_byte_len).unwrap();
    let forged = ScratchArtifact::with_bytes(&vec![0x5a; length]);

    // Even a catalog that would accept any signer (self-signed or a foreign
    // publisher) cannot turn bytes outside the packet claim into a signature.
    assert_eq!(
        verify_installed_authenticode(&AcceptingCatalog, &packet(), entry, &forged.0),
        Err(VerifyError::Artifact)
    );
    assert_eq!(
        verify_installed_authenticode(&WinVerifyTrustCatalog, &packet(), entry, &forged.0),
        Err(VerifyError::Artifact)
    );
}

#[cfg(windows)]
#[test]
fn live_win_verify_trust_rejects_a_signed_non_codex_publisher() {
    let windows = std::env::var_os("WINDIR").expect("Windows defines WINDIR");
    let notepad = PathBuf::from(windows).join("System32").join("notepad.exe");
    assert!(
        notepad.is_file(),
        "Windows ships a signed Notepad executable"
    );

    let result = authenticode_win_verify_trust(&notepad, INSTALLED_AUTHENTICODE_POLICY);

    assert_eq!(result, Err(VerifyError::Authenticode));
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
