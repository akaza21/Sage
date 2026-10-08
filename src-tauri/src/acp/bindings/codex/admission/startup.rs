use super::*;

pub(crate) fn startup_codex_binding_registration() -> CodexBindingRegistration {
    CodexBindingRegistration {
        authority: Arc::new(CodexRegistrationAuthority {
            state: Mutex::new(CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            )),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            fixture_only: false,
        }),
    }
}

pub(crate) fn admit_discovered_codex(
    registration: &CodexBindingRegistration,
    candidate: Option<crate::harness_discovery::InstalledHarness>,
) -> bool {
    candidate.is_some_and(|candidate| {
        crate::harness_discovery::revalidate_harness_version(candidate.id, &candidate.executable)
            .is_some_and(|version| {
                registration
                    .authenticate_observed(candidate.executable, version)
                    .is_ok()
            })
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
        let directory = std::env::temp_dir().join(format!("sage-discovered-codex-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        #[cfg(windows)]
        let executable = directory.join("codex.cmd");
        #[cfg(not(windows))]
        let executable = directory.join("codex");
        #[cfg(windows)]
        fs::write(&executable, b"@echo off\r\necho codex-cli 0.146.0\r\n").unwrap();
        #[cfg(not(windows))]
        {
            fs::write(&executable, b"#!/bin/sh\necho codex-cli 0.146.0\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        let registration = CodexBindingRegistration::unavailable_for_test();

        assert!(admit_discovered_codex(
            &registration,
            Some(crate::harness_discovery::InstalledHarness {
                id: "codex",
                executable: executable.clone(),
                version: Some("0.146.0".into()),
            }),
        ));
        let binding = registration.refresh().unwrap();
        assert!(binding.is_observed());
        assert_eq!(
            binding.entrypoint().path,
            executable.canonicalize().unwrap()
        );
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn display_version_cannot_admit_a_cli_that_fails_an_authoritative_version_probe() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("sage-cached-codex-{nonce}"));
        fs::create_dir_all(&directory).unwrap();
        #[cfg(windows)]
        let executable = directory.join("codex.cmd");
        #[cfg(not(windows))]
        let executable = directory.join("codex");
        #[cfg(windows)]
        fs::write(
            &executable,
            b"@echo off\r\necho codex-cli not-a-version\r\n",
        )
        .unwrap();
        #[cfg(not(windows))]
        {
            fs::write(&executable, b"#!/bin/sh\necho codex-cli not-a-version\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o700);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        let registration = CodexBindingRegistration::unavailable_for_test();

        assert!(!admit_discovered_codex(
            &registration,
            Some(crate::harness_discovery::InstalledHarness {
                id: "codex",
                executable,
                version: Some("0.146.0".into()),
            }),
        ));
        assert!(!registration.is_available());
        fs::remove_dir_all(directory).unwrap();
    }
}
