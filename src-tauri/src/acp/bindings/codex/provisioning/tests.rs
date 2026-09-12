use std::fs;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::*;

const FILES: [&str; 8] = [
    r"vendor\target\bin\codex.exe",
    r"vendor\target\bin\codex-code-mode-host.exe",
    r"vendor\target\resources\command-runner.exe",
    r"vendor\target\resources\sandbox.exe",
    r"vendor\target\path\rg.exe",
    r"vendor\target\codex-package.json",
    "package.json",
    "README.md",
];

struct TestProvisioning {
    base: PathBuf,
    source: PathBuf,
    package_target: PathBuf,
    state_target: PathBuf,
}

impl TestProvisioning {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-codex-provision-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let source = base.join("offline-source");
        let package_target = base.join("managed").join("package");
        let state_target = base.join("state").join("codex-app-server-fixture");
        fs::create_dir_all(package_target.parent().unwrap()).unwrap();
        fs::create_dir_all(state_target.parent().unwrap()).unwrap();
        for (index, relative) in FILES.iter().enumerate() {
            let path = source.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, [b'a' + index as u8]).unwrap();
        }
        Self {
            base,
            source: source.canonicalize().unwrap(),
            package_target,
            state_target,
        }
    }

    fn verified(&self) -> VerifiedCodexSource {
        self.verified_result().unwrap()
    }

    fn verified_result(&self) -> Result<VerifiedCodexSource, ProvisioningError> {
        VerifiedCodexSource::fixture(
            self.source.clone(),
            self.package_target.clone(),
            self.state_target.clone(),
            FILES
                .iter()
                .enumerate()
                .map(|(index, relative)| (PathBuf::from(relative), vec![b'a' + index as u8]))
                .collect(),
        )
    }

    fn stage_root(&self) -> PathBuf {
        self.package_target
            .parent()
            .unwrap()
            .join(".sage-codex-0.145.0-stage")
    }
}

impl Drop for TestProvisioning {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn exact_source_prepares_one_sealed_stage_without_publishing_targets() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();

    let prepared = prepare_codex_staging(&source, &fixture_authorization()).unwrap();

    assert_eq!(prepared.stage_root(), fixture.stage_root());
    assert!(prepared.stage_root().join("handoff.json").is_file());
    assert!(prepared.stage_root().join("package").is_dir());
    assert!(prepared.stage_root().join("state").is_dir());
    assert!(!fixture.package_target.exists());
    assert!(!fixture.state_target.exists());
    assert_eq!(prepared.revalidate(), Ok(()));
    assert_eq!(
        prepared.publish().unwrap_err(),
        ProvisioningError::AclFinalizationUnavailable
    );
    assert!(!fixture.stage_root().exists());
}

#[test]
fn sealed_handoff_binds_exact_files_targets_acl_plan_and_source_proof() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    let expected_proof = source.source_proof_sha256.clone();

    let prepared = prepare_codex_staging(&source, &fixture_authorization()).unwrap();
    let receipt: SealedHandoff =
        serde_json::from_slice(&fs::read(prepared.stage_root().join("handoff.json")).unwrap())
            .unwrap();

    assert_eq!(receipt.payload.schema_version, 1);
    assert_eq!(receipt.payload.binding_id, "codex-app-server");
    assert_eq!(receipt.payload.runtime_version, "0.145.0");
    assert_eq!(receipt.payload.source_proof_sha256, expected_proof);
    assert_eq!(receipt.payload.files.len(), 8);
    assert_eq!(receipt.payload.aggregate_byte_len, 8);
    assert_eq!(receipt.payload.package_target, fixture.package_target);
    assert_eq!(receipt.payload.state_target, fixture.state_target);
    assert!(!receipt.payload.creation_nonce.is_empty());
    assert!(!receipt.payload.current_user_sid_hex.is_empty());
    assert!(receipt.payload.package_acl.protected_dacl);
    assert!(receipt.payload.state_acl.protected_dacl);
    assert_eq!(
        receipt.payload.acl_application_status,
        "requiredPrivilegedHelperNotApplied"
    );
    assert_eq!(receipt.payload.publish_status, "notPublished");
}

#[test]
fn an_existing_canonical_target_blocks_staging_without_mutation() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    fs::create_dir(&fixture.package_target).unwrap();

    assert_eq!(
        prepare_codex_staging(&source, &fixture_authorization()).unwrap_err(),
        ProvisioningError::TargetExists
    );
    assert!(!fixture.stage_root().exists());
}

#[test]
fn an_existing_canonical_pair_selects_the_typed_update_gate() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    fs::create_dir(&fixture.package_target).unwrap();
    fs::create_dir(&fixture.state_target).unwrap();

    assert_eq!(
        windows::target_operation(&source),
        Ok(StagingOperation::Update)
    );
    assert!(!fixture.stage_root().exists());
}

#[test]
fn one_live_stage_blocks_a_second_transaction() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    let first = prepare_codex_staging(&source, &fixture_authorization()).unwrap();

    assert_eq!(
        prepare_codex_staging(&source, &fixture_authorization()).unwrap_err(),
        ProvisioningError::StageExists
    );
    assert_eq!(first.revalidate(), Ok(()));
}

#[test]
fn copy_failure_rolls_back_the_only_stage_and_leaves_targets_absent() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    windows::inject_copy_failure_after(3);

    assert_eq!(
        prepare_codex_staging(&source, &fixture_authorization()).unwrap_err(),
        ProvisioningError::Io
    );
    assert!(!fixture.stage_root().exists());
    assert!(!fixture.package_target.exists());
    assert!(!fixture.state_target.exists());
}

#[test]
fn receipt_drift_fails_revalidation_and_is_cleaned_on_drop() {
    let fixture = TestProvisioning::new();
    let source = fixture.verified();
    let mut prepared = prepare_codex_staging(&source, &fixture_authorization()).unwrap();

    prepared.corrupt_receipt_for_test();

    assert_eq!(prepared.revalidate(), Err(ProvisioningError::Receipt));
    drop(prepared);
    assert!(!fixture.stage_root().exists());
}

#[test]
fn hardlinked_offline_source_is_rejected_before_staging() {
    let fixture = TestProvisioning::new();
    fs::hard_link(
        fixture.source.join(FILES[0]),
        fixture.base.join("foreign-hard-link.exe"),
    )
    .unwrap();

    assert_eq!(
        fixture.verified_result().unwrap_err(),
        ProvisioningError::SourceBinding
    );
    assert!(!fixture.stage_root().exists());
}

#[test]
fn source_path_bound_rejects_exact_plus_one_before_inspection() {
    let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    let requirements = crate::acp::bindings::codex::parse_reproduction_requirements(
        include_bytes!("../../../../../testdata/acp/codex/package-reproduction-v1.json"),
        &packet,
    )
    .unwrap();
    let oversized = PathBuf::from(format!(r"C:\{}", "a".repeat(SOURCE_PATH_BYTES_MAX)));

    assert_eq!(
        verify_codex_source(requirements, &packet, oversized).unwrap_err(),
        ProvisioningError::SourcePath
    );
}

#[test]
#[ignore = "requires exact SAGE_CODEX_0145_OFFLINE_SOURCE_ROOT, explicit stage-only authorization, and absent canonical targets"]
fn exact_production_source_prepares_and_revalidates_helper_handoff() {
    let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    let requirements = crate::acp::bindings::codex::parse_reproduction_requirements(
        include_bytes!("../../../../../testdata/acp/codex/package-reproduction-v1.json"),
        &packet,
    )
    .unwrap();
    let source_root = PathBuf::from(
        std::env::var("SAGE_CODEX_0145_OFFLINE_SOURCE_ROOT")
            .expect("set the exact eight-file offline source root"),
    );
    let authorization = authorize_offline_staging_from_environment().unwrap();
    let source = verify_codex_source(requirements, &packet, source_root).unwrap();

    let prepared = prepare_codex_staging(&source, &authorization).unwrap();

    assert_eq!(prepared.revalidate(), Ok(()));
    assert_eq!(
        prepared.publish().unwrap_err(),
        ProvisioningError::AclFinalizationUnavailable
    );
}
