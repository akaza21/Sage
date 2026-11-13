use std::path::Path;

use super::{SignedInstalledRole, admit_installed_role};
use crate::acp::bindings::codex::manifest::MetadataValidatedPacket;
use crate::acp::bindings::codex::parse_packet_metadata;
use crate::acp::bindings::codex::reproduction::authenticode_unsigned_relative_paths;
use crate::acp::bindings::codex::trust_root::{
    AuthenticodeCatalog, INSTALLED_AUTHENTICODE_POLICY, InstalledAuthenticodePolicy, VerifyError,
    WinVerifyTrustCatalog, bind_packet_artifact, verify_installed_authenticode,
    verify_installed_authenticode_for_observed_digest,
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

/// Runs the production admission chain for one installed executable: the
/// role is reachable only through a `VerifiedSignature` minted by
/// `verify_installed_authenticode` with the WinVerifyTrust catalog.
fn admit_executable(
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
    executable: &Path,
) -> Option<SignedInstalledRole> {
    let signature = verify_installed_authenticode(
        &WinVerifyTrustCatalog,
        packet,
        artifact_relative_path,
        executable,
    )
    .ok()?;
    admit_installed_role(packet, signature).ok()
}

#[test]
fn untrusted_executables_never_acquire_an_installed_role() {
    let packet = parse_packet_metadata(PACKET_FIXTURE).unwrap();
    let directory = std::env::temp_dir().join(format!("sage-role-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();

    let runner = r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-command-runner.exe";
    let runner_len = packet
        .physical_artifact_claims()
        .find(|(_, relative)| *relative == runner)
        .map(|(artifact, _)| usize::try_from(artifact.byte_len).unwrap())
        .unwrap();
    let forged_runner = directory.join("codex-command-runner.exe");
    std::fs::write(&forged_runner, vec![0x5a; runner_len]).unwrap();

    let unsigned_tool = authenticode_unsigned_relative_paths()[0];
    let replaced_tool = directory.join("rg.exe");
    std::fs::write(
        &replaced_tool,
        b"MZ replacement for an unsigned packaged tool",
    )
    .unwrap();

    let foreign = directory.join("foreign.exe");
    std::fs::write(&foreign, b"MZ executable that the packet never claimed").unwrap();

    let missing = directory.join("missing-codex.exe");

    let cases = [
        (runner, forged_runner.as_path()),
        (unsigned_tool, replaced_tool.as_path()),
        ("foreign.exe", foreign.as_path()),
        (SIGNED_ENTRY, missing.as_path()),
    ];
    for (claimed, executable) in cases {
        assert_eq!(
            admit_executable(&packet, claimed, executable),
            None,
            "{claimed} must not be admitted from {}",
            executable.display()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
