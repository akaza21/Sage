use std::path::Path;

use super::admit_installed_role;
use crate::acp::bindings::codex::parse_packet_metadata;
use crate::acp::bindings::codex::trust_root::{
    AuthenticodeCatalog, INSTALLED_AUTHENTICODE_POLICY, InstalledAuthenticodePolicy, VerifyError,
    bind_packet_artifact, verify_installed_authenticode_for_observed_digest,
};

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

#[test]
fn admission_requires_a_packet_bound_verified_signature() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let binding = bind_packet_artifact(&packet, SIGNED_ENTRY).unwrap();
    let signature = verify_installed_authenticode_for_observed_digest(
        &AcceptingCatalog,
        &packet,
        SIGNED_ENTRY,
        Path::new("codex.exe"),
        binding.artifact_sha256(),
    )
    .unwrap();
    let admitted = admit_installed_role(&packet, signature).unwrap();
    assert_eq!(admitted.binding_id, packet.metadata().binding_id);
    assert_eq!(admitted.artifact_relative_path, SIGNED_ENTRY);
}

#[test]
fn verified_signature_has_no_stringly_constructor() {
    let source = include_str!("../trust_root.rs");
    assert!(!source.contains("impl From<&str>"));
    assert!(!source.contains("impl From<String>"));
    assert!(source.contains("binding: PacketArtifactBinding"));
}
