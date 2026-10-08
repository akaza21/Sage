use super::super::installed_role::SignedInstalledRole;
use super::*;
use sha2::{Digest, Sha256};
use std::io::Read;

impl AdmittedCodexBinding {
    fn admit_for_test(
        packet: MetadataValidatedPacket,
        reproduced: ReproducedPackagePrerequisite,
        authority: CodexAuthorityPrerequisite,
        environment: CodexLaunchEnvironment,
        cleanup: TestCleanup,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        let entrypoint = entrypoint_identity(&packet)?;
        let artifact_digest = packet.metadata().entrypoint.sha256.clone();
        let harness_version = packet.metadata().codex_runtime_version.clone();
        let binding = Self {
            packet,
            reproduced: Some(reproduced),
            authority: Some(authority),
            environment: Some(environment),
            entrypoint,
            observed: None,
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            desktop_fixture: None,
            binding_digest: BINDING_DIGEST,
            binding_digest_hex: BINDING_DIGEST_HEX.into(),
            artifact_digest,
            harness_version,
            characterisation:
                crate::acp::bindings::characterisation::Characterisation::Characterised,
            journey_test_entrypoint: None,
            journey_test_cases: None,
            _cleanup: Some(cleanup),
        };
        binding.revalidate()?;
        Ok(binding)
    }
}

#[derive(Debug)]
pub(super) struct TestCleanup(Vec<PathBuf>);

impl Drop for TestCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub(super) fn admitted_binding_fixture() -> AdmittedCodexBinding {
    let (packet, _, reproduced, authority, environment, cleanup) = signed_role_admission_fixture();
    AdmittedCodexBinding::admit_for_test(packet, reproduced, authority, environment, cleanup)
        .unwrap()
}

pub(super) fn admitted_journey_binding_fixture(
    case_ids: &'static [&'static str],
) -> AdmittedCodexBinding {
    admitted_journey_binding_fixture_from(case_ids, &journey_fixture_source())
}

fn admitted_journey_binding_fixture_from(
    case_ids: &'static [&'static str],
    source: &std::path::Path,
) -> AdmittedCodexBinding {
    let mut binding = admitted_binding_fixture();
    let (entrypoint, mut cleanup) = journey_fixture_entrypoint(source);
    binding.journey_test_entrypoint = Some(entrypoint);
    binding._cleanup.as_mut().unwrap().0.append(&mut cleanup.0);
    binding.journey_test_cases = Some(Mutex::new(case_ids.iter().copied().collect()));
    binding
}

pub(super) fn admitted_observed_steer_journey_fixture(
    case_ids: &'static [&'static str],
) -> AdmittedCodexBinding {
    admitted_observed_steer_journey_fixture_from(case_ids, &journey_fixture_source())
}

fn admitted_observed_steer_journey_fixture_from(
    case_ids: &'static [&'static str],
    source: &std::path::Path,
) -> AdmittedCodexBinding {
    let (entrypoint, cleanup) = journey_fixture_entrypoint(source);
    let mut binding =
        AdmittedCodexBinding::admit_observed(entrypoint.path.clone(), "codex 0.146.0".into())
            .expect("observed fixture admission");
    binding.journey_test_entrypoint = Some(entrypoint);
    binding.journey_test_cases = Some(Mutex::new(case_ids.iter().copied().collect()));
    binding._cleanup = Some(cleanup);
    binding
}

fn journey_fixture_source() -> PathBuf {
    std::env::current_exe()
        .expect("journey test executable")
        .parent()
        .expect("journey test executable directory")
        .parent()
        .expect("journey target directory")
        .join("acp-adversarial-fixture.exe")
}

fn journey_fixture_entrypoint(source: &std::path::Path) -> (EntrypointIdentity, TestCleanup) {
    let directory = std::env::temp_dir().join(format!(
        "sage-codex-journey-fixture-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&directory).expect("journey fixture directory");
    let cleanup = TestCleanup(vec![directory.clone()]);
    let staged = directory.join("acp-adversarial-fixture.exe");
    std::fs::copy(source, &staged).expect("stage journey fixture");
    let path = staged.canonicalize().expect("already-built ACP fixture");
    let mut file = std::fs::File::open(&path).expect("fixture artifact");
    let byte_len = file.metadata().expect("fixture metadata").len();
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut chunk).expect("fixture read");
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    (
        EntrypointIdentity {
            path,
            sha256: digest.finalize().into(),
            byte_len,
        },
        cleanup,
    )
}

#[test]
#[cfg_attr(not(feature = "acp-fixture"), ignore = "requires acp-fixture")]
fn journey_bindings_keep_admitted_fixture_bytes_after_source_replacement() {
    let parent = std::env::temp_dir().join(format!(
        "sage-codex-journey-source-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&parent).unwrap();
    let _source_cleanup = TestCleanup(vec![parent.clone()]);
    let source = parent.join("fixture.exe");
    std::fs::copy(journey_fixture_source(), &source).unwrap();

    let observed = admitted_observed_steer_journey_fixture_from(&[], &source);
    let signed = admitted_journey_binding_fixture_from(&[], &source);
    std::fs::write(&source, b"a concurrent build replaced the source").unwrap();

    observed
        .revalidate()
        .expect("observed admission owns fixture bytes");
    for binding in [&observed, &signed] {
        let entrypoint = binding.journey_test_entrypoint.as_ref().unwrap();
        let current = super::verify::observed_entrypoint_identity(&entrypoint.path).unwrap();
        assert_eq!(current.sha256, entrypoint.sha256);
        assert_eq!(current.byte_len, entrypoint.byte_len);
    }
}

type SignedRoleAdmissionFixture = (
    MetadataValidatedPacket,
    SignedInstalledRole,
    ReproducedPackagePrerequisite,
    CodexAuthorityPrerequisite,
    CodexLaunchEnvironment,
    TestCleanup,
);

fn signed_role_admission_fixture() -> SignedRoleAdmissionFixture {
    let parent =
        std::env::temp_dir().join(format!("sage-codex-admission-{}", uuid::Uuid::new_v4()));
    signed_role_admission_fixture_at(parent)
}

fn canonical_fixture_root(parent: &std::path::Path) -> PathBuf {
    use std::path::{Component, Prefix};

    std::fs::create_dir_all(parent).unwrap();
    let canonical = parent.canonicalize().unwrap();
    let mut components = canonical.components();
    let drive = match components.next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => panic!("fixture root must resolve to a local Windows drive"),
        },
        _ => panic!("fixture root must resolve to an absolute Windows path"),
    };
    assert_eq!(components.next(), Some(Component::RootDir));
    let mut canonical = PathBuf::from(format!("{}:\\", char::from(drive)));
    canonical.extend(components);
    canonical
}

fn signed_role_admission_fixture_at(parent: PathBuf) -> SignedRoleAdmissionFixture {
    use serde_json::{Value, json};

    const PACKET: &[u8] =
        include_bytes!("../../../../../testdata/acp/codex/candidate-windows.json");
    const POLICY: &[u8] =
        include_bytes!("../../../../../testdata/acp/codex/security-policy-v1.json");
    const CONFIG: &[u8] = include_bytes!("../../../../../testdata/acp/codex/config-v1.toml");

    let parent = canonical_fixture_root(&parent);
    let home = parent
        .join("CodexState")
        .join("codex-app-server-0.146.0-d3992fec");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    std::fs::write(home.join("config.toml"), CONFIG).unwrap();

    let mut packet_value: Value = serde_json::from_slice(PACKET).unwrap();
    packet_value["codexHome"] = json!(home.to_str().unwrap());
    let packet =
        super::super::parse_packet_metadata(&serde_json::to_vec(&packet_value).unwrap()).unwrap();
    let mut policy_value: Value = serde_json::from_slice(POLICY).unwrap();
    policy_value["codexHome"] = json!(home.to_str().unwrap());
    let policy =
        super::super::parse_security_policy(&serde_json::to_vec(&policy_value).unwrap(), &packet)
            .unwrap();
    let config = super::super::verify_codex_config(CONFIG, &policy).unwrap();
    let reproduced = super::super::reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = super::super::managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);
    let environment =
        super::super::authority::build_system_codex_launch_environment(&packet, &policy, &config)
            .unwrap();
    let authority =
        super::super::inspect_codex_authority(&packet, policy, config, &reproduced, managed)
            .unwrap();
    struct AcceptingCatalog;

    impl super::super::trust_root::AuthenticodeCatalog for AcceptingCatalog {
        fn win_verify_trust_file(
            &self,
            _path: &std::path::Path,
            _policy: super::super::trust_root::InstalledAuthenticodePolicy,
        ) -> Result<(), super::super::trust_root::VerifyError> {
            Ok(())
        }
    }

    let signature = super::super::trust_root::verify_installed_authenticode_for_observed_digest(
        &AcceptingCatalog,
        &packet,
        r"vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        std::path::Path::new("codex.exe"),
        &packet.metadata().entrypoint.sha256,
    )
    .unwrap();
    let role = super::super::installed_role::admit_installed_role(&packet, signature).unwrap();

    (
        packet,
        role,
        reproduced,
        authority,
        environment,
        TestCleanup(vec![parent]),
    )
}

#[test]
fn shared_admission_fixture_canonicalizes_root_before_deriving_authority() {
    let raw_parent = std::env::temp_dir().join(format!(
        "sage-codex-admission-alias-{}",
        uuid::Uuid::new_v4()
    ));
    let aliased_parent = raw_parent.join("lexical-alias").join("..");
    let expected_parent = canonical_fixture_root(&aliased_parent);
    assert_ne!(aliased_parent, expected_parent);

    let (packet, role, reproduced, authority, environment, cleanup) =
        signed_role_admission_fixture_at(aliased_parent);
    let expected_home = expected_parent
        .join("CodexState")
        .join("codex-app-server-0.146.0-d3992fec");
    assert_eq!(cleanup.0, vec![expected_parent]);
    assert_eq!(packet.codex_home(), expected_home);
    assert_eq!(
        environment
            .entries()
            .iter()
            .find(|(key, _)| key == "CODEX_HOME")
            .map(|(_, value)| value.as_str()),
        expected_home.to_str()
    );
    reproduced.revalidate(&packet).unwrap();
    authority.revalidate(&packet, &reproduced).unwrap();
    environment.revalidate(&packet).unwrap();
    assert_eq!(role.binding_id, packet.metadata().binding_id);
    let expected_temp = expected_home.join("tmp");
    for key in ["TEMP", "TMP"] {
        assert_eq!(
            environment
                .entries()
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.as_str()),
            expected_temp.to_str()
        );
    }

    let registration = CodexBindingRegistration::unavailable_for_test();
    let admitted = registration
        .authenticate_signed_installed_role(&packet, role, reproduced, authority, environment)
        .unwrap();
    admitted.revalidate().unwrap();
    assert!(Arc::ptr_eq(&admitted, &registration.refresh().unwrap()));
}

#[test]
fn authenticate_signed_installed_role_rejects_a_mismatched_artifact_role() {
    let rejected_registration = CodexBindingRegistration {
        authority: Arc::new(CodexRegistrationAuthority {
            state: Mutex::new(CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            )),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            fixture_only: false,
        }),
    };
    let (packet, mut unbound_role, reproduced, authority, environment, _cleanup) =
        signed_role_admission_fixture();

    unbound_role.artifact_relative_path =
        r"vendor\x86_64-pc-windows-msvc\codex-path\rg.exe".to_owned();
    assert_eq!(
        rejected_registration
            .authenticate_signed_installed_role(
                &packet,
                unbound_role,
                reproduced,
                authority,
                environment,
            )
            .unwrap_err(),
        CodexAdmissionUnavailable::IndependentAdmission
    );
    assert_eq!(
        crate::harnesses::HarnessCatalog::with_codex_registration_and_presence(
            rejected_registration.clone(),
            crate::acp::bindings::codex::CodexHostPresence::Absent,
        )
        .settled_for_test()
        .definitions()
        .remove(0)
        .install_state,
        "notInstalled"
    );

    let registration = CodexBindingRegistration::unavailable_for_test();
    let (packet, role, reproduced, authority, environment, _cleanup) =
        signed_role_admission_fixture();
    let admitted = registration
        .authenticate_signed_installed_role(&packet, role, reproduced, authority, environment)
        .unwrap();

    assert!(registration.is_available());
    assert!(Arc::ptr_eq(&admitted, &registration.refresh().unwrap()));
    assert_eq!(
        crate::harnesses::HarnessCatalog::with_codex_registration(registration.clone())
            .settled_for_test()
            .definitions()
            .remove(0)
            .install_state,
        "available"
    );
    assert!(registration.same_authority(&registration.clone()));
    assert!(!registration.same_authority(&startup_codex_binding_registration()));
}

#[test]
fn workstation_authenticode_observation_refuses_independent_launch() {
    let registration = CodexBindingRegistration::unavailable_for_test();

    assert_eq!(
        registration.refuse_independent_launch_after_workstation_authenticode_observation(),
        CodexAdmissionUnavailable::ObservationOnly
    );
    assert_eq!(
        registration.refresh().unwrap_err(),
        CodexAdmissionUnavailable::ObservationOnly
    );
}
