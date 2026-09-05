use super::*;

pub(crate) fn startup_codex_binding_registration() -> CodexBindingRegistration {
    let registration = CodexBindingRegistration {
        authority: Arc::new(CodexRegistrationAuthority {
            state: Mutex::new(CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            )),
        }),
    };
    if !admit_discovered_codex(
        &registration,
        crate::harness_discovery::discover_installed_harnesses()
            .into_iter()
            .find(|candidate| candidate.id == "codex"),
    ) {
        #[cfg(windows)]
        {
            let _ = authenticate_startup_signed_installed_role(&registration);
        }
    }
    registration
}

fn admit_discovered_codex(
    registration: &CodexBindingRegistration,
    candidate: Option<crate::harness_discovery::InstalledHarness>,
) -> bool {
    candidate.is_some_and(|candidate| {
        registration
            .authenticate_observed(candidate.executable, candidate.version)
            .is_ok()
    })
}

#[cfg(windows)]
fn authenticate_startup_signed_installed_role(
    registration: &CodexBindingRegistration,
) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
    let packet = super::super::parse_packet_metadata(include_bytes!(
        "../../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let requirements = super::super::parse_reproduction_requirements(
        include_bytes!("../../../../../testdata/acp/codex/package-reproduction-v1.json"),
        &packet,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    // Parsing the pinned requirements validates that the recorded Authenticode
    // status is exactly `workstationObservationOnly`. That observation can only
    // refine a failure of the independent physical-admission step below.
    requirements
        .revalidate(&packet)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let role = packet.require_physical_admission().map_err(|_| {
        registration.refuse_independent_launch_after_workstation_authenticode_observation()
    })?;
    let root = super::super::ConfiguredManagedPackageRoot::new(packet.managed_package_root())
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let inspection = super::super::inspect_managed_package(&packet, &root)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let reproduced = super::super::bind_reproduced_package(requirements, &packet, inspection)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let managed = super::super::inspect_managed_staging(&packet, &reproduced)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let policy = super::super::parse_security_policy(
        include_bytes!("../../../../../testdata/acp/codex/security-policy-v1.json"),
        &packet,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let config = super::super::verify_codex_config(
        include_bytes!("../../../../../testdata/acp/codex/config-v1.toml"),
        &policy,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let environment =
        super::super::authority::build_system_codex_launch_environment(&packet, &policy, &config)
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let authority =
        super::super::inspect_codex_authority(&packet, policy, config, &reproduced, managed)
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;

    registration.authenticate_signed_installed_role(
        &packet,
        role,
        reproduced,
        authority,
        environment,
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn startup_promotes_a_runnable_discovered_codex_cli() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let executable = std::env::temp_dir().join(format!("sage-discovered-codex-{nonce}.bin"));
        fs::write(&executable, b"observed Codex CLI").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();

        assert!(admit_discovered_codex(
            &registration,
            Some(crate::harness_discovery::InstalledHarness {
                id: "codex",
                executable: executable.clone(),
                version: "codex 0.146.0".into(),
            }),
        ));
        let binding = registration.refresh().unwrap();
        assert!(binding.is_observed());
        assert_eq!(
            binding.entrypoint().path,
            executable.canonicalize().unwrap()
        );
        fs::remove_file(executable).unwrap();
    }
}
