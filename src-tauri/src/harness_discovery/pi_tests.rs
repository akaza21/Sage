use super::*;

fn npm_root(name: &str, bin: &str) -> PathBuf {
    npm_root_with_entry(name, bin, "dist/cli.js")
}

fn npm_root_with_entry(name: &str, bin: &str, entry: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sage-pi-discovery-{}", uuid::Uuid::new_v4()));
    let package = root.join(package_relative(""));
    let entry = package.join(entry);
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(entry, b"#!/usr/bin/env node\n").unwrap();
    std::fs::write(
        package.join("package.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": name,
            "version": "0.74.2",
            "bin": {"pi": bin},
            "engines": {"node": ">=20.6.0"}
        }))
        .unwrap(),
    )
    .unwrap();
    root.canonicalize().unwrap()
}

#[test]
fn a_missing_package_is_absent() {
    assert_eq!(
        discover_pi_launch_from(Vec::new(), Vec::new(), ProbeLimits::PRODUCTION),
        PiLaunchDiscovery::Absent
    );
}

#[test]
fn a_foreign_package_or_bin_is_never_admitted() {
    for (name, bin) in [
        ("@other/pi-coding-agent", "dist/cli.js"),
        (crate::acp::bindings::pi::PI_PACKAGE, "dist/other.js"),
        (
            crate::acp::bindings::pi::PI_PACKAGE,
            "../pi-coding-agent/dist/cli.js",
        ),
        (crate::acp::bindings::pi::PI_PACKAGE, "/dist/cli.js"),
        (crate::acp::bindings::pi::PI_PACKAGE, "dist/cli.ts"),
        (crate::acp::bindings::pi::PI_PACKAGE, ""),
    ] {
        let root = npm_root(name, bin);
        assert_eq!(
            discover_pi_launch_from(vec![root.clone()], Vec::new(), ProbeLimits::PRODUCTION),
            PiLaunchDiscovery::PackageUnreadable
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn the_curated_package_without_node_reports_node_absent() {
    let root = npm_root(crate::acp::bindings::pi::PI_PACKAGE, "dist/cli.js");
    assert_eq!(
        discover_pi_launch_from(vec![root.clone()], Vec::new(), ProbeLimits::PRODUCTION),
        PiLaunchDiscovery::NodeAbsent
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A Node the version probe can run. Unix runs a script, so the test needs no
/// host install; Windows launches only a real `node.exe`, so it takes the
/// first host Node that discovery itself would admit.
#[cfg(not(windows))]
fn admissible_node(root: &Path) -> Option<PathBuf> {
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    Some(crate::harness_discovery::tests::write_script(
        &bin,
        "node",
        "echo v22.12.0",
    ))
}

#[cfg(windows)]
fn admissible_node(_root: &Path) -> Option<PathBuf> {
    node_candidates()
        .into_iter()
        .find(|path| super::super::validation::authentic_harness_file(path, "node.exe"))
}

#[test]
fn the_curated_package_with_node_is_present_with_digests() {
    let root = npm_root(crate::acp::bindings::pi::PI_PACKAGE, "./dist/cli.js");
    let Some(node) = admissible_node(&root) else {
        std::fs::remove_dir_all(root).unwrap();
        return;
    };
    let found =
        match discover_pi_launch_from(vec![root.clone()], vec![node], ProbeLimits::PRODUCTION) {
            PiLaunchDiscovery::Present(found) => found,
            other => panic!("the curated package with Node must be present: {other:?}"),
        };
    assert_eq!(found.package_version, "0.74.2");
    assert_eq!(found.node_engine, ">=20.6.0");
    #[cfg(not(windows))]
    assert_eq!(found.node_version, "22.12.0");
    assert_eq!(found.entrypoint_sha256.len(), 64);
    assert!(found.entrypoint_path.ends_with("dist/cli.js"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_entrypoint_is_the_bin_the_installed_release_declares() {
    // Pi 1.x moved its `pi` bin to the bundled build.
    let root = npm_root_with_entry(
        crate::acp::bindings::pi::PI_PACKAGE,
        "dist/bundle/cli.js",
        "dist/bundle/cli.js",
    );
    let Some(node) = admissible_node(&root) else {
        std::fs::remove_dir_all(root).unwrap();
        return;
    };
    let found =
        match discover_pi_launch_from(vec![root.clone()], vec![node], ProbeLimits::PRODUCTION) {
            PiLaunchDiscovery::Present(found) => found,
            other => panic!("the declared bin must be found: {other:?}"),
        };
    assert!(found.entrypoint_path.ends_with("dist/bundle/cli.js"));
    std::fs::remove_dir_all(root).unwrap();
}
