use super::*;

#[test]
fn the_registry_routes_pi_to_its_curated_read_only_binding() {
    let registry = BindingRegistry::default();
    let record = registry.for_harness("pi").expect("Pi has a curated record");
    assert_eq!(record.binding_id(), "pi-rpc");
    assert!(registry.route("pi-rpc").is_ok());
    // Discovery is pending until the CLI is found; Absent is a reason.
    assert!(!record.admission_available());
    registry
        .pi_admission()
        .apply_discovery(crate::harness_discovery::PiLaunchDiscovery::Absent);
    assert_eq!(
        record.unavailable_reason(),
        Some(crate::domain::HarnessUnavailableReason::CandidateInvalid)
    );
    assert!(registry.resolve("pi-rpc").is_err());
}
