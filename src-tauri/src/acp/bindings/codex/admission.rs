#[cfg(all(test, windows))]
use std::collections::VecDeque;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::{
    authority::{CodexAuthorityPrerequisite, CodexLaunchEnvironment},
    manifest::{MetadataValidatedPacket, pinned_codex_version, pinned_schema_sha256},
    reproduction::ReproducedPackagePrerequisite,
};
use crate::acp::bindings::characterisation::Characterisation;

pub(crate) const BINDING_ID: &str = "codex-app-server";
const BINDING_DIGEST_INPUT: &[u8] = concat!(
    "sage:codex-app-server:",
    pinned_codex_version!(),
    ":windows-x86_64:",
    pinned_schema_sha256!()
)
.as_bytes();
/// SHA-256 of [`BINDING_DIGEST_INPUT`].
const BINDING_DIGEST: [u8; 32] = [
    0xcb, 0xb9, 0x05, 0x88, 0x5f, 0x1f, 0xa3, 0xeb, 0xce, 0x55, 0x00, 0x10, 0xf3, 0xd6, 0x72, 0xd7,
    0x19, 0x7b, 0x03, 0x2b, 0xd4, 0x5b, 0xe3, 0xd9, 0xc3, 0xfa, 0x71, 0x04, 0x4d, 0xb9, 0x90, 0x71,
];
const BINDING_DIGEST_HEX: &str = "cbb905885f1fa3ebce550010f3d672d7197b032bd45be3d9c3fa71044db99071";

#[derive(Debug)]
pub(crate) struct EntrypointIdentity {
    pub(crate) path: PathBuf,
    pub(crate) sha256: [u8; 32],
    pub(crate) byte_len: u64,
}

#[derive(Debug)]
pub(crate) struct AdmittedCodexBinding {
    packet: MetadataValidatedPacket,
    reproduced: Option<ReproducedPackagePrerequisite>,
    authority: Option<CodexAuthorityPrerequisite>,
    environment: Option<CodexLaunchEnvironment>,
    entrypoint: EntrypointIdentity,
    observed: Option<ObservedEntrypoint>,
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    desktop_fixture: Option<DesktopCodexFixture>,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
    artifact_digest: String,
    harness_version: String,
    characterisation: Characterisation,
    #[cfg(all(test, windows))]
    journey_test_entrypoint: Option<EntrypointIdentity>,
    #[cfg(all(test, windows))]
    journey_test_cases: Option<Mutex<VecDeque<&'static str>>>,
    #[cfg(all(test, windows))]
    _cleanup: Option<authenticate_tests::TestCleanup>,
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[derive(Debug)]
struct DesktopCodexFixture {
    selection: crate::launch_profile::CodexFixtureSelection,
    environment: Vec<(String, String)>,
}

#[derive(Debug)]
struct ObservedEntrypoint {
    inputs: Arc<dyn ObservedLaunchInputs>,
    environment: Vec<(String, String)>,
}

mod environment;
#[cfg(test)]
use environment::provider_environment_keys;
use environment::{ObservedLaunchInputs, SystemLaunchInputs, observed_environment};
#[cfg(test)]
mod environment_tests;

mod record;
mod report;
mod verify;

pub(crate) use report::CodexAdmissionUnavailable;
#[cfg(all(test, windows))]
use verify::entrypoint_identity;

mod registration;
pub(crate) use registration::{
    CodexBindingRegistration, CodexRegistrationAuthority, CodexRegistrationState,
};

mod startup;
pub(crate) use startup::{admit_discovered_codex, startup_codex_binding_registration};

#[cfg(test)]
mod test_registration;

#[cfg(all(test, windows))]
mod authenticate_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn the_binding_digest_is_the_hash_of_the_pinned_binding_identity() {
        use sha2::{Digest, Sha256};

        let digest: [u8; 32] = Sha256::digest(BINDING_DIGEST_INPUT).into();
        assert_eq!(digest, BINDING_DIGEST);
        assert_eq!(super::super::provisioning::hex(&digest), BINDING_DIGEST_HEX);
    }

    #[test]
    fn observed_binding_revalidates_the_exact_discovered_file_identity() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sage-observed-codex-{nonce}.bin"));
        fs::write(&path, b"observed-codex").expect("test executable");
        let binding =
            AdmittedCodexBinding::admit_observed_for_test(path.clone(), "codex 1.2.3".into())
                .expect("observed executable admission");
        assert_eq!(binding.entrypoint().path, path.canonicalize().unwrap());
        binding.revalidate().expect("unchanged executable");
        fs::write(&path, b"changed-codex").expect("change test executable");
        assert_eq!(binding.revalidate(), Err(CodexAdmissionUnavailable::Drift));
        fs::remove_file(path).expect("remove test executable");
    }

    #[test]
    fn explicit_builtin_openai_without_a_provider_table_is_admitted() {
        assert_eq!(
            provider_environment_keys("model_provider = \"openai\""),
            Ok(Vec::new())
        );
    }

    #[test]
    fn explicit_builtin_openai_table_preserves_its_declared_environment_keys() {
        assert_eq!(
            provider_environment_keys(
                r#"model_provider = "openai"
[model_providers.openai]
env_key = "OPENAI_OVERRIDE_KEY"
env_http_headers = { "X-Extra" = "OPENAI_HEADER_KEY" }
"#,
            ),
            Ok(vec![
                "OPENAI_HEADER_KEY".into(),
                "OPENAI_OVERRIDE_KEY".into()
            ])
        );
    }

    #[test]
    fn undefined_providers_are_rejected() {
        assert_eq!(
            provider_environment_keys("model_provider = \"unknown\""),
            Err(CodexAdmissionUnavailable::IndependentAdmission)
        );
    }

    #[test]
    fn malformed_selected_provider_configuration_is_rejected() {
        for config in [
            "model_provider = [",
            "model_provider = 42",
            "model_provider = \"openai\"\nmodel_providers = 42",
            "model_provider = \"openai\"\n[model_providers]\nopenai = 42",
            "model_provider = \"openai\"\n[model_providers.openai]\nenv_key = 42",
            "model_provider = \"openai\"\n[model_providers.openai]\nenv_key = \"bad-key\"",
            "model_provider = \"openai\"\n[model_providers.openai]\nenv_http_headers = 42",
            "model_provider = \"openai\"\n[model_providers.openai.env_http_headers]\nX = 42",
        ] {
            assert_eq!(
                provider_environment_keys(config),
                Err(CodexAdmissionUnavailable::IndependentAdmission),
                "{config}"
            );
        }
    }

    #[test]
    fn observed_provider_environment_uses_only_declared_keys_for_the_selected_provider() {
        let keys = provider_environment_keys(
            r#"
model_provider = "cliproxyapi"

[model_providers.cliproxyapi]
env_key = "CLIPROXY_API_KEY"
env_http_headers = { "X-Api-Key" = "CLIPROXY_API_KEY", "X-Secondary-Key" = "SECONDARY_PROXY_KEY" }

[model_providers.unselected]
env_key = "UNSELECTED_PROVIDER_KEY"
"#,
        )
        .expect("provider configuration");

        assert_eq!(keys, ["CLIPROXY_API_KEY", "SECONDARY_PROXY_KEY"]);
    }

    #[test]
    fn admission_failure_reports_remain_redacted_and_stable() {
        assert_eq!(
            CodexAdmissionUnavailable::Platform.to_string(),
            "Codex Harness Admission is unavailable on this platform"
        );
        assert_eq!(
            CodexAdmissionUnavailable::ObservationOnly.to_string(),
            "the workstation can only observe the Codex Authenticode role"
        );
        assert_eq!(
            CodexAdmissionUnavailable::IndependentAdmission.to_string(),
            "the exact Codex Binding has not completed independent Harness Admission"
        );
        assert_eq!(
            CodexAdmissionUnavailable::Drift.to_string(),
            "the admitted Codex Binding changed after admission"
        );
    }

    #[test]
    fn startup_registration_is_typed_unavailable_or_refreshable_after_independent_admission() {
        let registration = startup_codex_binding_registration();
        let separately_started = startup_codex_binding_registration();

        assert!(!registration.same_authority(&separately_started));
        #[cfg(not(windows))]
        match registration.unavailable() {
            Some(CodexAdmissionUnavailable::IndependentAdmission) => {}
            None => assert!(registration.is_available()),
            Some(unavailable) => panic!("unexpected startup availability: {unavailable:?}"),
        }
        #[cfg(windows)]
        match registration.unavailable() {
            Some(CodexAdmissionUnavailable::ObservationOnly) => {}
            Some(CodexAdmissionUnavailable::IndependentAdmission) => {}
            Some(unavailable) => panic!("unexpected startup availability: {unavailable:?}"),
            None => {
                assert!(registration.is_available());
                registration.refresh().unwrap();
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn admitted_binding_owns_and_revalidates_every_launch_prerequisite() {
        let binding = authenticate_tests::admitted_binding_fixture();

        binding.revalidate().unwrap();
        assert_eq!(binding.binding_id(), "codex-app-server");
        assert_eq!(binding.binding_digest_hex(), BINDING_DIGEST_HEX);
        let entrypoint = &binding.packet().metadata().entrypoint;
        assert_eq!(binding.artifact_digest(), entrypoint.sha256);
        assert_eq!(binding.entrypoint().byte_len, entrypoint.byte_len);
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_refreshes_converge_on_one_fail_closed_drift_transition() {
        let registration = CodexBindingRegistration::admitted_for_test();
        registration.induce_drift_for_test();
        let gate = Arc::new(std::sync::Barrier::new(5));
        let workers = (0..4)
            .map(|_| {
                let registration = registration.clone();
                let gate = Arc::clone(&gate);
                std::thread::spawn(move || {
                    gate.wait();
                    registration.refresh().err()
                })
            })
            .collect::<Vec<_>>();
        gate.wait();

        for worker in workers {
            assert_eq!(
                worker.join().unwrap(),
                Some(CodexAdmissionUnavailable::Drift)
            );
        }
        assert!(!registration.is_available());
        assert_eq!(
            registration.unavailable(),
            Some(CodexAdmissionUnavailable::Drift)
        );
    }
}
