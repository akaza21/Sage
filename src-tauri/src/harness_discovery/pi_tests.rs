use super::*;

fn npm_root(name: &str, bin: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sage-pi-discovery-{}", uuid::Uuid::new_v4()));
    let package = root.join(package_relative(""));
    std::fs::create_dir_all(package.join("dist")).unwrap();
    std::fs::write(package.join("dist/cli.js"), b"#!/usr/bin/env node\n").unwrap();
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

#[test]
fn the_curated_package_with_node_is_present_with_digests() {
    let nodes: Vec<PathBuf> = node_candidates()
        .into_iter()
        .filter(|path| path.is_file())
        .collect();
    if nodes.is_empty() {
        return;
    }
    let root = npm_root(crate::acp::bindings::pi::PI_PACKAGE, "./dist/cli.js");
    let found = match discover_pi_launch_from(vec![root.clone()], nodes, ProbeLimits::PRODUCTION) {
        PiLaunchDiscovery::Present(found) => found,
        other => panic!("the curated package with Node must be present: {other:?}"),
    };
    assert_eq!(found.package_version, "0.74.2");
    assert_eq!(found.node_engine, ">=20.6.0");
    assert_eq!(found.entrypoint_sha256.len(), 64);
    assert!(found.entrypoint_path.ends_with("dist/cli.js"));
    std::fs::remove_dir_all(root).unwrap();
}
