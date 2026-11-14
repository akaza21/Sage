use std::fs;
use std::path::{Path, PathBuf};

use crate::acp::process::BoundedEnvironment;
use crate::project_registry::ProjectRootAuthority;
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId, SageRootId};

use super::{AcpLaunchAuthority, admit, candidate_from_global_npm_shim, profile_for};

const MANIFEST_TEST_VERSION: &str = "1.18.21";

struct TempTree(PathBuf);

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest_bytes(version: &str) -> String {
    format!(
        r#"{{"name":"opencode-ai","version":"{version}","bin":{{"opencode":"./bin/opencode.exe"}}}}"#
    )
}

fn npm_tree(label: &str) -> (TempTree, PathBuf, PathBuf) {
    let root = TempTree(sage_test_support::test_paths::temp_dir().join(format!(
        "sage-acp-manifest-{label}-{}",
        uuid::Uuid::new_v4()
    )));
    let npm = root.0.join("npm");
    let package = npm.join("node_modules").join("opencode-ai");
    let runtime = package.join("bin").join("opencode.exe");
    fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
    fs::write(&runtime, b"native-opencode").expect("write runtime");
    let manifest = package.join("package.json");
    fs::write(&manifest, manifest_bytes(MANIFEST_TEST_VERSION)).expect("write manifest");
    let shim = npm.join("opencode.cmd");
    fs::write(&shim, b"shim").expect("write shim");
    (root, shim, manifest)
}

fn launch_spec(label: &str, shim: &Path, project_dir: &Path) -> crate::acp::process::LaunchSpec {
    let binding = admit(
        profile_for("opencode").expect("OpenCode profile"),
        candidate_from_global_npm_shim(shim, MANIFEST_TEST_VERSION.into()).expect("candidate"),
    )
    .expect("admitted binding");
    let project_root = ProjectRootAuthority::for_test(
        SageProjectId::parse(format!("project-{label}")).expect("project"),
        ProjectGeneration::ZERO,
        SageRootId::parse(format!("root-{label}")).expect("root"),
        project_dir,
    )
    .expect("project root");
    AcpLaunchAuthority::from_admitted_binding(
        &binding,
        11,
        BoundedEnvironment::new(Vec::new()).expect("environment"),
    )
    .expect("launch authority")
    .into_launch_spec(project_root)
    .expect("launch spec")
}

#[test]
fn opencode_launch_spec_refuses_a_package_manifest_substituted_after_admission() {
    let (root, shim, manifest) = npm_tree("substitution");
    let spec = launch_spec("acp-manifest-substitution", &shim, &root.0);
    let expectation = spec
        .artifact
        .manifest
        .as_ref()
        .expect("the launch spec carries the revalidated package manifest");
    assert_eq!(expectation.path(), manifest.as_path());
    assert!(expectation.open_guard().is_ok());

    drop(expectation.open_guard().expect("guard before substitution"));
    fs::write(&manifest, manifest_bytes("9.9.9")).expect("substitute manifest");

    assert!(
        spec.artifact
            .manifest
            .as_ref()
            .expect("manifest expectation")
            .open_guard()
            .is_err()
    );
}

#[cfg(windows)]
#[test]
fn held_manifest_guard_denies_writers_and_deleters_across_process_creation() {
    let (root, shim, manifest) = npm_tree("deny-write");
    let spec = launch_spec("acp-manifest-deny-write", &shim, &root.0);
    let guard = spec
        .artifact
        .manifest
        .as_ref()
        .expect("manifest expectation")
        .open_guard()
        .expect("manifest guard");

    assert!(fs::write(&manifest, manifest_bytes("9.9.9")).is_err());
    assert!(fs::remove_file(&manifest).is_err());

    drop(guard);
    assert!(fs::write(&manifest, manifest_bytes("9.9.9")).is_ok());
}

#[cfg(windows)]
#[test]
fn manifest_guard_refuses_a_byte_identical_reparse_point_substituted_after_admission() {
    // WinBase.h volume flag, independent of junction-creation permissions.
    const FILE_SUPPORTS_REPARSE_POINTS: u32 = 0x0000_0080;
    let (root, shim, manifest) = npm_tree("reparse");
    let supports_junctions =
        sage_test_support::test_paths::volume_flags(&root.0) & FILE_SUPPORTS_REPARSE_POINTS != 0;
    let spec = launch_spec("acp-manifest-reparse", &shim, &root.0);
    let expectation = spec
        .artifact
        .manifest
        .as_ref()
        .expect("manifest expectation");
    drop(expectation.open_guard().expect("guard before substitution"));

    let package = manifest.parent().expect("package directory").to_path_buf();
    let decoy = root.0.join("decoy");
    fs::create_dir_all(&decoy).expect("create decoy package");
    fs::copy(&manifest, decoy.join("package.json")).expect("copy byte identical manifest");
    fs::rename(&package, root.0.join("relocated-opencode-ai")).expect("relocate real package");
    let junction = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&package)
        .arg(&decoy)
        .status()
        .expect("create directory junction");
    if supports_junctions {
        assert!(junction.success());
        assert_eq!(
            fs::read(&manifest).ok(),
            fs::read(decoy.join("package.json")).ok()
        );
    } else {
        assert!(!junction.success(), "this volume must refuse a junction");
        assert!(!manifest.exists());
        assert_eq!(
            expectation.open_guard().err(),
            Some(sage_process::error::LaunchError::Io)
        );
    }

    assert!(expectation.open_guard().is_err());
}
