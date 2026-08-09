use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::*;

const FIXTURE_FILES: [(&str, &[u8], &str); 8] = [
    (
        r"vendor\target\bin\codex.exe",
        b"a",
        "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
    ),
    (
        r"vendor\target\bin\codex-code-mode-host.exe",
        b"b",
        "3e23e8160039594a33894f6564e1b1348bbd7a0088d42c4acb73eeaed59c009d",
    ),
    (
        r"vendor\target\resources\command-runner.exe",
        b"c",
        "2e7d2c03a9507ae265ecf5b5356885a53393a2029d241394997265a1a25aefc6",
    ),
    (
        r"vendor\target\resources\sandbox.exe",
        b"d",
        "18ac3e7343f016890c510e93f935261169d9e3f565436429830faf0934f4f8e4",
    ),
    (
        r"vendor\target\path\rg.exe",
        b"e",
        "3f79bb7b435b05321651daefd374cdc681dc06faa65e374e38337b88ca046dea",
    ),
    (
        r"vendor\target\codex-package.json",
        b"f",
        "252f10c83610ebca1a059c0bae8255eba2f95be4d1d7bcfa89d7248a82d9f111",
    ),
    (
        "package.json",
        b"g",
        "cd0aa9856147b6c5b4ff2b7dfee5da20aa38253099ef1b4a64aced233c9afe29",
    ),
    (
        "README.md",
        b"h",
        "aaa9402664f1a41f40ebbc52c9993eb66aeb366602958fdfaa283b71e64db123",
    ),
];

struct TestPackage {
    base: PathBuf,
    package: PathBuf,
    codex_home: PathBuf,
}

struct CustomPackage {
    base: PathBuf,
    package: PathBuf,
    codex_home: PathBuf,
    files: Vec<ExpectedPhysicalFile>,
}

impl CustomPackage {
    fn with_paths(paths: Vec<PathBuf>) -> Self {
        assert_eq!(paths.len(), EXPECTED_FILE_COUNT);
        let base = std::env::temp_dir().join(format!("sage-codex-bounds-{}", Uuid::new_v4()));
        let package = base.join("package");
        let digest =
            decode_digest("ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb")
                .unwrap();
        let mut files = Vec::with_capacity(paths.len());
        for relative_path in paths {
            let path = package.join(&relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"a").unwrap();
            files.push(ExpectedPhysicalFile {
                relative_path,
                sha256: digest,
                byte_len: 1,
            });
        }
        let package = package.canonicalize().unwrap();
        Self {
            codex_home: base.join("state"),
            base,
            package,
            files,
        }
    }

    fn claims(&self) -> InspectionClaims {
        InspectionClaims {
            package_root: self.package.clone(),
            codex_home: self.codex_home.clone(),
            files: self.files.clone(),
            aggregate_bytes: EXPECTED_FILE_COUNT as u64,
        }
    }
}

impl Drop for CustomPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

impl TestPackage {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-codex-physical-{}", Uuid::new_v4()));
        let package = base.join("package");
        for (relative, bytes, _) in FIXTURE_FILES {
            let path = package.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        let package = package.canonicalize().unwrap();
        Self {
            codex_home: base.join("state").join("codex-app-server-fixture"),
            base,
            package,
        }
    }

    fn claims(&self) -> InspectionClaims {
        InspectionClaims {
            package_root: self.package.clone(),
            codex_home: self.codex_home.clone(),
            files: FIXTURE_FILES
                .iter()
                .map(|(relative, _, digest)| ExpectedPhysicalFile {
                    relative_path: PathBuf::from(relative),
                    sha256: decode_digest(digest).unwrap(),
                    byte_len: 1,
                })
                .collect(),
            aggregate_bytes: 8,
        }
    }
}

impl Drop for TestPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn exact_eight_file_closure_returns_a_non_launch_proof_without_creating_state() {
    let fixture = TestPackage::new();
    let proof = inspect_claims(fixture.claims()).unwrap();
    assert_eq!(proof.package_root(), fixture.package.as_path());
    assert_eq!(proof.artifact_count(), EXPECTED_FILE_COUNT);
    assert_eq!(proof.byte_len(), 8);
    assert!(!fixture.codex_home.exists());
}

#[test]
fn missing_extra_size_digest_and_aggregate_drift_all_fail_closed() {
    let missing = TestPackage::new();
    fs::remove_file(missing.package.join("README.md")).unwrap();
    assert_eq!(
        inspect_claims(missing.claims()).unwrap_err(),
        PhysicalInspectionError::Closure
    );

    let extra = TestPackage::new();
    fs::write(extra.package.join("unexpected"), b"x").unwrap();
    assert_eq!(
        inspect_claims(extra.claims()).unwrap_err(),
        PhysicalInspectionError::Closure
    );

    let size = TestPackage::new();
    fs::write(size.package.join("README.md"), b"hh").unwrap();
    assert_eq!(
        inspect_claims(size.claims()).unwrap_err(),
        PhysicalInspectionError::Identity
    );

    let digest = TestPackage::new();
    fs::write(digest.package.join("README.md"), b"i").unwrap();
    assert_eq!(
        inspect_claims(digest.claims()).unwrap_err(),
        PhysicalInspectionError::Digest
    );

    let aggregate = TestPackage::new();
    let mut claims = aggregate.claims();
    claims.aggregate_bytes += 1;
    assert_eq!(
        inspect_claims(claims).unwrap_err(),
        PhysicalInspectionError::Closure
    );
}

#[test]
fn hard_link_and_file_reparse_aliases_are_rejected() {
    let hard_link = TestPackage::new();
    fs::hard_link(
        hard_link.package.join("README.md"),
        hard_link.base.join("outside-hard-link"),
    )
    .unwrap();
    assert_eq!(
        inspect_claims(hard_link.claims()).unwrap_err(),
        PhysicalInspectionError::Alias
    );

    let reparse = TestPackage::new();
    let target = reparse.base.join("outside-target");
    fs::write(&target, b"h").unwrap();
    let linked = reparse.package.join("README.md");
    fs::remove_file(&linked).unwrap();
    if std::os::windows::fs::symlink_file(&target, &linked).is_ok() {
        assert_eq!(
            inspect_claims(reparse.claims()).unwrap_err(),
            PhysicalInspectionError::Alias
        );
    }
}

#[test]
fn codex_home_equal_nested_parent_and_alias_paths_are_rejected_without_mutation() {
    for home in [
        |fixture: &TestPackage| fixture.package.clone(),
        |fixture: &TestPackage| fixture.package.join("state"),
        |fixture: &TestPackage| fixture.base.clone(),
    ] {
        let fixture = TestPackage::new();
        let mut claims = fixture.claims();
        claims.codex_home = home(&fixture);
        let existed = claims.codex_home.exists();
        assert_eq!(
            inspect_claims(claims).unwrap_err(),
            PhysicalInspectionError::StatePath
        );
        assert_eq!(home(&fixture).exists(), existed);
    }

    let state_file = TestPackage::new();
    let path = state_file.base.join("state-file");
    fs::write(&path, b"preserve").unwrap();
    let mut claims = state_file.claims();
    claims.codex_home = path.clone();
    assert_eq!(
        inspect_claims(claims).unwrap_err(),
        PhysicalInspectionError::StatePath
    );
    assert_eq!(fs::read(path).unwrap(), b"preserve");
}

#[test]
fn an_existing_open_writer_prevents_inspection() {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    let fixture = TestPackage::new();
    let target = fixture.package.join("README.md");
    let mut writer = OpenOptions::new()
        .write(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&target)
        .unwrap();
    writer.seek(SeekFrom::Start(0)).unwrap();
    assert_eq!(
        inspect_claims(fixture.claims()).unwrap_err(),
        PhysicalInspectionError::Io
    );
}

#[test]
fn a_post_hash_identity_race_is_rejected_before_proof() {
    let fixture = TestPackage::new();
    let target = fixture.package.join("README.md");
    super::windows::inject_after_hash(target, |_| {
        super::windows::force_next_info_change();
    });
    assert_eq!(
        inspect_claims(fixture.claims()).unwrap_err(),
        PhysicalInspectionError::ChangedDuringInspection
    );
}

#[test]
fn configured_package_root_must_match_every_packet_path_exactly() {
    let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    let wrong = ConfiguredManagedPackageRoot::new(PathBuf::from(r"C:\Other\package")).unwrap();
    assert_eq!(
        inspect_managed_package(&packet, &wrong).unwrap_err(),
        PhysicalInspectionError::PacketRootMismatch
    );

    let exact_path = PathBuf::from(r"C:\SageHarnesses\Codex\0.145.0\package");
    let exact = ConfiguredManagedPackageRoot::new(exact_path.clone()).unwrap();
    let claims = super::windows::claims_from_packet(&packet, &exact).unwrap();
    assert_eq!(claims.package_root, exact_path);
    assert_eq!(claims.files.len(), EXPECTED_FILE_COUNT);
    assert_eq!(claims.aggregate_bytes, 427_151_500);
}

#[test]
fn traversal_entry_and_depth_limits_accept_exact_and_reject_plus_one() {
    let exact_depth_paths = std::iter::once(PathBuf::from(r"a\b\c\d\e\f\g\deep"))
        .chain((0..7).map(|index| PathBuf::from(format!("root-{index}"))))
        .collect();
    let exact_depth = CustomPackage::with_paths(exact_depth_paths);
    assert!(inspect_claims(exact_depth.claims()).is_ok());

    let plus_one_depth_paths = std::iter::once(PathBuf::from(r"a\b\c\d\e\f\g\h\too-deep"))
        .chain((0..7).map(|index| PathBuf::from(format!("root-{index}"))))
        .collect();
    let plus_one_depth = CustomPackage::with_paths(plus_one_depth_paths);
    assert_eq!(
        inspect_claims(plus_one_depth.claims()).unwrap_err(),
        PhysicalInspectionError::DepthLimit
    );

    let exact_entry_paths = (0..EXPECTED_FILE_COUNT)
        .map(|index| PathBuf::from(format!(r"root-{index}\a\b\c\d\e\f\file")))
        .collect();
    let exact_entries = CustomPackage::with_paths(exact_entry_paths);
    assert!(inspect_claims(exact_entries.claims()).is_ok());

    let plus_one_entries_paths = (0..EXPECTED_FILE_COUNT)
        .map(|index| PathBuf::from(format!(r"root-{index}\a\b\c\d\e\f\file")))
        .collect();
    let plus_one_entries = CustomPackage::with_paths(plus_one_entries_paths);
    fs::write(plus_one_entries.package.join("entry-65"), b"x").unwrap();
    assert_eq!(
        inspect_claims(plus_one_entries.claims()).unwrap_err(),
        PhysicalInspectionError::EntryLimit
    );
}

#[test]
fn configured_root_is_bounded_and_root_or_state_reparse_aliases_fail_closed() {
    let oversized = PathBuf::from(format!(r"C:\{}", "a".repeat(CONFIGURED_PATH_BYTES_MAX)));
    assert_eq!(
        ConfiguredManagedPackageRoot::new(oversized).unwrap_err(),
        PhysicalInspectionError::ConfiguredRoot
    );

    let root_alias = TestPackage::new();
    let linked_root = root_alias.base.join("package-link");
    if std::os::windows::fs::symlink_dir(&root_alias.package, &linked_root).is_ok() {
        let mut claims = root_alias.claims();
        claims.package_root = linked_root;
        assert_eq!(
            inspect_claims(claims).unwrap_err(),
            PhysicalInspectionError::Alias
        );
    }

    let state_alias = TestPackage::new();
    let state_target = state_alias.base.join("state-target");
    fs::create_dir(&state_target).unwrap();
    let linked_state = state_alias.base.join("state-link");
    if std::os::windows::fs::symlink_dir(&state_target, &linked_state).is_ok() {
        let mut claims = state_alias.claims();
        claims.codex_home = linked_state;
        assert_eq!(
            inspect_claims(claims).unwrap_err(),
            PhysicalInspectionError::StatePath
        );
    }
}
