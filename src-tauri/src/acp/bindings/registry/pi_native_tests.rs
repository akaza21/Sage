use super::*;

#[test]
fn the_registry_routes_pi_to_its_curated_read_only_binding() {
    let registry = BindingRegistry::default();
    let record = registry.for_harness("pi").expect("Pi has a curated record");
    assert_eq!(record.binding_id(), "pi-rpc");
    assert!(registry.route("pi-rpc").is_ok());
    assert!(!record.admission_available());
    registry
        .pi_admission()
        .apply_discovery(crate::harness_discovery::PiLaunchDiscovery::Absent);
    assert_eq!(
        record.unavailable_reason(),
        Some(sage_domain::harness::HarnessUnavailableReason::CandidateInvalid)
    );
    assert!(registry.resolve("pi-rpc").is_err());
}

#[test]
fn isolated_titles_require_the_characterised_pi_build() {
    use crate::harness_discovery::{DiscoveredPiLaunch, PiLaunchDiscovery};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../work")
        .join(format!("pi-title-registry-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let node = root.join("node.exe");
    let entry = root.join("cli.js");
    std::fs::write(&node, b"fixture node").unwrap();
    std::fs::write(&entry, b"fixture pi").unwrap();
    for (version, isolated) in [("1.5.0", true), ("1.6.0", false)] {
        let registry = BindingRegistry::default();
        registry
            .pi_admission()
            .apply_discovery(PiLaunchDiscovery::Present(Box::new(DiscoveredPiLaunch {
                entrypoint_path: entry.clone(),
                node_path: node.clone(),
                package_version: version.into(),
                node_version: "22.19.0".into(),
                node_engine: ">=22.19.0".into(),
                node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
                entrypoint_sha256: crate::harness_discovery::sha256_file(&entry).unwrap(),
            })));
        let record = registry
            .resolve("pi-rpc")
            .unwrap_or_else(|_| panic!("admitted Pi"));
        assert_eq!(
            matches!(record.title_capability(), TitleCapability::GenerateIsolated),
            isolated
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
