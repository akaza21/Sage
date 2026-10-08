#[cfg(all(test, windows))]
use std::collections::VecDeque;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use super::{
    authority::{CodexAuthorityPrerequisite, CodexLaunchEnvironment},
    manifest::MetadataValidatedPacket,
    reproduction::ReproducedPackagePrerequisite,
};
use crate::acp::bindings::characterisation::Characterisation;

pub(crate) const BINDING_ID: &str = "codex-app-server";
const BINDING_DIGEST_INPUT: &[u8] = b"sage:codex-app-server:0.146.0:windows-x86_64:d3992fec1398afdbec658da2c720c6993fbf3c1ce4900785694d2196679eddfc";
const BINDING_DIGEST: [u8; 32] = [
    0x3b, 0xba, 0x7d, 0x86, 0xd2, 0x35, 0x7d, 0x8f, 0xcc, 0xd8, 0x1b, 0xdb, 0xa3, 0xd7, 0xf3, 0xf4,
    0x27, 0x8c, 0x80, 0x01, 0x4d, 0x7e, 0x65, 0xe0, 0x85, 0xb9, 0xa8, 0x74, 0x33, 0x0d, 0x7d, 0xf4,
];
const BINDING_DIGEST_HEX: &str = "3bba7d86d2357d8fccd81bdba3d7f3f4278c80014d7e65e085b9a874330d7df4";

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
    environment: Vec<(String, String)>,
}

fn observed_environment(
    entrypoint: &std::path::Path,
) -> Result<Vec<(String, String)>, CodexAdmissionUnavailable> {
    #[cfg(not(windows))]
    {
        let _ = entrypoint;
        const KEYS: &[&str] = &[
            "HOME",
            "PATH",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ];
        KEYS.iter()
            .filter_map(|key| {
                std::env::var(key)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(|value| ((*key).to_owned(), value))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .try_fold(Vec::new(), |mut entries, entry| {
                if entry.0.len() > 128 || entry.1.len() > 8_192 {
                    return Err(CodexAdmissionUnavailable::IndependentAdmission);
                }
                entries.push(entry);
                Ok(entries)
            })
    }
    #[cfg(windows)]
    {
        const KEYS: &[&str] = &[
            "APPDATA",
            "ComSpec",
            "PATHEXT",
            "SystemRoot",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "WINDIR",
        ];
        let mut entries = KEYS
            .iter()
            .map(|key| {
                std::env::var(key)
                    .ok()
                    .filter(|value| !value.is_empty())
                    .map(|value| ((*key).to_owned(), value))
                    .ok_or(CodexAdmissionUnavailable::IndependentAdmission)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let node_directory = entrypoint
            .parent()
            .into_iter()
            .map(std::path::Path::to_path_buf)
            .chain(std::env::var("PATH").ok().into_iter().flat_map(|path| {
                path.split(';')
                    .filter(|value| !value.is_empty())
                    .map(std::path::PathBuf::from)
                    .collect::<Vec<_>>()
            }))
            .find(|directory| directory.join("node.exe").is_file())
            .and_then(|directory| directory.to_str().map(str::to_owned))
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        entries.insert(0, ("PATH".into(), node_directory));
        for key in configured_provider_environment_keys()? {
            if let Some(value) = std::env::var(&key).ok().filter(|value| !value.is_empty()) {
                entries.push((key, value));
            }
        }
        if entries.len() > 16
            || entries
                .iter()
                .any(|(key, value)| key.len() > 128 || value.len() > 8_192)
        {
            return Err(CodexAdmissionUnavailable::IndependentAdmission);
        }
        Ok(entries)
    }
}

fn configured_provider_environment_keys() -> Result<Vec<String>, CodexAdmissionUnavailable> {
    let home = std::env::var("USERPROFILE")
        .map(PathBuf::from)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let path = home.join(".codex").join("config.toml");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(CodexAdmissionUnavailable::IndependentAdmission),
    };
    if bytes.len() > 64 * 1024 {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    provider_environment_keys(
        std::str::from_utf8(&bytes).map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?,
    )
}

fn provider_environment_keys(config: &str) -> Result<Vec<String>, CodexAdmissionUnavailable> {
    let config = config
        .parse::<toml::Value>()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let provider = match config.get("model_provider").and_then(toml::Value::as_str) {
        Some(provider) => provider,
        None => return Ok(Vec::new()),
    };
    let provider = config
        .get("model_providers")
        .and_then(toml::Value::as_table)
        .and_then(|providers| providers.get(provider))
        .and_then(toml::Value::as_table)
        .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
    let mut keys = BTreeSet::new();
    if let Some(key) = provider.get("env_key").and_then(toml::Value::as_str) {
        insert_provider_environment_key(&mut keys, key)?;
    }
    if let Some(headers) = provider
        .get("env_http_headers")
        .and_then(toml::Value::as_table)
    {
        for key in headers.values().filter_map(toml::Value::as_str) {
            insert_provider_environment_key(&mut keys, key)?;
        }
    }
    Ok(keys.into_iter().collect())
}

fn insert_provider_environment_key(
    keys: &mut BTreeSet<String>,
    key: &str,
) -> Result<(), CodexAdmissionUnavailable> {
    if key.is_empty()
        || key.len() > 128
        || !key.bytes().enumerate().all(|(index, byte)| {
            byte == b'_'
                || byte.is_ascii_alphanumeric() && (index > 0 || byte.is_ascii_alphabetic())
        })
    {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    keys.insert(key.to_owned());
    Ok(())
}

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
    fn observed_binding_revalidates_the_exact_discovered_file_identity() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sage-observed-codex-{nonce}.bin"));
        fs::write(&path, b"observed-codex").expect("test executable");
        let binding = AdmittedCodexBinding::admit_observed(path.clone(), "codex 1.2.3".into())
            .expect("observed executable admission");
        assert_eq!(binding.entrypoint().path, path.canonicalize().unwrap());
        binding.revalidate().expect("unchanged executable");
        fs::write(&path, b"changed-codex").expect("change test executable");
        assert_eq!(binding.revalidate(), Err(CodexAdmissionUnavailable::Drift));
        fs::remove_file(path).expect("remove test executable");
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
        assert_eq!(
            binding.binding_digest_hex(),
            "3bba7d86d2357d8fccd81bdba3d7f3f4278c80014d7e65e085b9a874330d7df4"
        );
        assert_eq!(
            binding.artifact_digest(),
            "bc343ba420dc2e2e9f59e6fc5e5bf0aae1cd8c771fc319665241fc9c0271fddb"
        );
        assert_eq!(binding.entrypoint().byte_len, 358_650_672);
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
