use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::*;
use crate::acp_protocol::codex_protocol::PINNED_CODEX_VERSION;

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

/// Content of the files that pad the named fixtures out to the pinned
/// package's file count.
const PADDING_FILE_BYTES: &[u8] = b"x";

fn padding_files() -> impl Iterator<Item = PathBuf> {
    (FIXTURE_FILES.len()..EXPECTED_FILE_COUNT).map(|index| {
        PathBuf::from(format!(
            r"vendor\target\resources\voice\bin\padding-{index}.dll"
        ))
    })
}

/// Paths for exactly the pinned file count whose files and distinct parent
/// directories together make `entries` traversal entries.
fn paths_with_entry_count(entries: usize) -> Vec<PathBuf> {
    let mut directories = entries - EXPECTED_FILE_COUNT;
    let paths = (0..EXPECTED_FILE_COUNT)
        .map(|index| {
            let depth = directories.min(PACKAGE_DEPTH_MAX - 1);
            directories -= depth;
            let mut path = (0..depth)
                .map(|level| format!("root-{index}-{level}"))
                .collect::<PathBuf>();
            path.push(format!("file-{index}"));
            path
        })
        .collect();
    assert_eq!(directories, 0, "{entries} entries fit the depth bound");
    paths
}

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

fn canonical_fixture_base(prefix: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    base.canonicalize().unwrap()
}

impl CustomPackage {
    fn with_paths(paths: Vec<PathBuf>) -> Self {
        assert_eq!(paths.len(), EXPECTED_FILE_COUNT);
        let base = canonical_fixture_base("sage-codex-bounds");
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
        let base = canonical_fixture_base("sage-codex-physical");
        let package = base.join("package");
        let named = FIXTURE_FILES
            .iter()
            .map(|(relative, bytes, _)| (PathBuf::from(relative), *bytes));
        let padding = padding_files().map(|relative| (relative, PADDING_FILE_BYTES));
        for (relative, bytes) in named.chain(padding) {
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
                .chain(padding_files().map(|relative_path| ExpectedPhysicalFile {
                    relative_path,
                    sha256: Sha256::digest(PADDING_FILE_BYTES).into(),
                    byte_len: 1,
                }))
                .collect(),
            aggregate_bytes: EXPECTED_FILE_COUNT as u64,
        }
    }
}

impl Drop for TestPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn exact_pinned_file_closure_returns_a_non_launch_proof_without_creating_state() {
    let fixture = TestPackage::new();
    let proof = inspect_claims(fixture.claims()).unwrap();
    assert_eq!(proof.package_root(), fixture.package.as_path());
    assert_eq!(proof.artifact_count(), EXPECTED_FILE_COUNT);
    assert_eq!(proof.byte_len(), EXPECTED_FILE_COUNT as u64);
    assert!(!fixture.codex_home.exists());
}

#[test]
fn fixture_package_and_state_paths_are_derived_from_the_same_canonical_base() {
    let fixture = TestPackage::new();
    assert_eq!(fixture.base.canonicalize().unwrap(), fixture.base);
    assert_eq!(fixture.package, fixture.base.join("package"));
    assert_eq!(
        fixture.codex_home,
        fixture.base.join("state").join("codex-app-server-fixture")
    );

    let custom_paths = (0..EXPECTED_FILE_COUNT)
        .map(|index| PathBuf::from(format!("root-{index}")))
        .collect();
    let custom = CustomPackage::with_paths(custom_paths);
    assert_eq!(custom.base.canonicalize().unwrap(), custom.base);
    assert_eq!(custom.package, custom.base.join("package"));
    assert_eq!(custom.codex_home, custom.base.join("state"));
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
fn physical_component_spelling_must_exactly_match_the_declared_closure() {
    let file = TestPackage::new();
    let declared = file.package.join("README.md");
    let different_spelling = file.package.join("README.MD");
    fs::remove_file(&declared).unwrap();
    fs::write(&different_spelling, b"h").unwrap();

    assert_eq!(
        inspect_claims(file.claims()).unwrap_err(),
        PhysicalInspectionError::Closure
    );

    let directory = TestPackage::new();
    let temporary = directory.package.join("vendor-temporary");
    fs::rename(directory.package.join("vendor"), &temporary).unwrap();
    fs::rename(temporary, directory.package.join("VENDOR")).unwrap();
    assert_eq!(
        inspect_claims(directory.claims()).unwrap_err(),
        PhysicalInspectionError::Closure
    );
}

#[test]
fn hard_link_is_rejected() {
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
}

#[test]
fn file_symlink_smoke_rejects_when_the_os_permits_fixture_creation() {
    let reparse = TestPackage::new();
    let target = reparse.base.join("outside-target");
    fs::write(&target, b"h").unwrap();
    let linked = reparse.package.join("README.md");
    fs::remove_file(&linked).unwrap();
    if let Err(error) = std::os::windows::fs::symlink_file(&target, &linked) {
        eprintln!("optional Windows file-symlink smoke unavailable: {error}");
        return;
    }
    assert_eq!(
        inspect_claims(reparse.claims()).unwrap_err(),
        PhysicalInspectionError::Alias
    );
}

#[test]
fn codex_home_equal_nested_parent_and_file_paths_are_rejected_without_mutation() {
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
fn dangling_state_reparse_is_not_treated_as_a_missing_component() {
    let fixture = TestPackage::new();
    let dangling = fixture.base.join("dangling-state-link");
    std::os::windows::fs::symlink_dir(fixture.base.join("missing-target"), &dangling)
        .expect("Windows physical-verifier tests require unprivileged symlink support");
    let mut claims = fixture.claims();
    claims.codex_home = dangling.join("child");

    assert_eq!(
        inspect_claims(claims).unwrap_err(),
        PhysicalInspectionError::StatePath
    );
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
fn an_entry_added_after_initial_enumeration_is_rejected_before_proof() {
    let fixture = TestPackage::new();
    let late_entry = fixture.package.join("late-entry");
    super::windows::inject_before_namespace_recheck(move || {
        fs::write(late_entry, b"late").unwrap();
    });

    assert_eq!(
        inspect_claims(fixture.claims()).unwrap_err(),
        PhysicalInspectionError::ChangedDuringInspection
    );
}

#[test]
fn configured_package_root_and_packet_relative_spelling_must_match_exactly() {
    let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .unwrap();
    let wrong = ConfiguredManagedPackageRoot::new(PathBuf::from(r"C:\Other\package")).unwrap();
    assert_eq!(
        inspect_managed_package(&packet, &wrong).unwrap_err(),
        PhysicalInspectionError::PacketRootMismatch
    );
    let wrong_component_case = ConfiguredManagedPackageRoot::new(PathBuf::from(format!(
        r"C:\sageHarnesses\Codex\{PINNED_CODEX_VERSION}\package"
    )))
    .unwrap();
    assert_eq!(
        super::windows::claims_from_packet(&packet, &wrong_component_case).unwrap_err(),
        PhysicalInspectionError::PacketRootMismatch
    );

    let exact_path = PathBuf::from(format!(
        r"C:\SageHarnesses\Codex\{PINNED_CODEX_VERSION}\package"
    ));
    let exact = ConfiguredManagedPackageRoot::new(exact_path.clone()).unwrap();
    let claims = super::windows::claims_from_packet(&packet, &exact).unwrap();
    assert_eq!(claims.package_root, exact_path);
    assert_eq!(claims.files.len(), EXPECTED_FILE_COUNT);
    assert_eq!(
        claims.aggregate_bytes,
        packet.metadata().package_unpacked_byte_len
    );

    let represented = ConfiguredManagedPackageRoot::new(PathBuf::from(format!(
        r"\\?\c:\SageHarnesses\Codex\{PINNED_CODEX_VERSION}\package"
    )))
    .unwrap();
    assert!(super::windows::claims_from_packet(&packet, &represented).is_ok());

    let wrong_case = String::from_utf8(
        include_bytes!("../../../../../testdata/acp/codex/candidate-windows.json").to_vec(),
    )
    .unwrap()
    .replace("README.md", "README.MD");
    let wrong_case_packet =
        crate::acp::bindings::codex::parse_packet_metadata(wrong_case.as_bytes()).unwrap();
    assert_eq!(
        super::windows::claims_from_packet(&wrong_case_packet, &exact).unwrap_err(),
        PhysicalInspectionError::PacketRootMismatch
    );
}

#[test]
fn traversal_entry_and_depth_limits_accept_exact_and_reject_plus_one() {
    let exact_depth_paths = std::iter::once(PathBuf::from(r"a\b\c\d\e\f\g\deep"))
        .chain((1..EXPECTED_FILE_COUNT).map(|index| PathBuf::from(format!("root-{index}"))))
        .collect();
    let exact_depth = CustomPackage::with_paths(exact_depth_paths);
    assert!(inspect_claims(exact_depth.claims()).is_ok());

    let plus_one_depth_paths = std::iter::once(PathBuf::from(r"a\b\c\d\e\f\g\h\too-deep"))
        .chain((1..EXPECTED_FILE_COUNT).map(|index| PathBuf::from(format!("root-{index}"))))
        .collect();
    let plus_one_depth = CustomPackage::with_paths(plus_one_depth_paths);
    assert_eq!(
        inspect_claims(plus_one_depth.claims()).unwrap_err(),
        PhysicalInspectionError::DepthLimit
    );

    let exact_entries = CustomPackage::with_paths(paths_with_entry_count(PACKAGE_ENTRY_MAX));
    assert!(inspect_claims(exact_entries.claims()).is_ok());

    let plus_one_entries = CustomPackage::with_paths(paths_with_entry_count(PACKAGE_ENTRY_MAX));
    fs::write(plus_one_entries.package.join("entry-plus-one"), b"x").unwrap();
    assert_eq!(
        inspect_claims(plus_one_entries.claims()).unwrap_err(),
        PhysicalInspectionError::EntryLimit
    );
}

#[test]
fn configured_root_is_bounded_and_optional_root_or_state_symlink_smokes_fail_closed() {
    let oversized = PathBuf::from(format!(r"C:\{}", "a".repeat(CONFIGURED_PATH_BYTES_MAX)));
    assert_eq!(
        ConfiguredManagedPackageRoot::new(oversized).unwrap_err(),
        PhysicalInspectionError::ConfiguredRoot
    );

    let root_alias = TestPackage::new();
    let linked_root = root_alias.base.join("package-link");
    match std::os::windows::fs::symlink_dir(&root_alias.package, &linked_root) {
        Ok(()) => {
            let mut claims = root_alias.claims();
            claims.package_root = linked_root;
            assert_eq!(
                inspect_claims(claims).unwrap_err(),
                PhysicalInspectionError::Alias
            );
        }
        Err(error) => eprintln!("optional Windows root-symlink smoke unavailable: {error}"),
    }

    let state_alias = TestPackage::new();
    let state_target = state_alias.base.join("state-target");
    fs::create_dir(&state_target).unwrap();
    let linked_state = state_alias.base.join("state-link");
    match std::os::windows::fs::symlink_dir(&state_target, &linked_state) {
        Ok(()) => {
            let mut claims = state_alias.claims();
            claims.codex_home = linked_state;
            assert_eq!(
                inspect_claims(claims).unwrap_err(),
                PhysicalInspectionError::StatePath
            );
        }
        Err(error) => eprintln!("optional Windows state-symlink smoke unavailable: {error}"),
    }
}
