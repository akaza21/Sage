use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::harness_discovery::{
    InstalledHarness, authentic_harness_file, authentic_npm_global_descendant, sha256_file,
};
use crate::{
    acp::{
        bindings::{
            adapter::AdapterRuntimeError,
            runtime::{NativeRuntime, RuntimeFactory},
        },
        contract::{RuntimeEpoch, SageAgentSessionId},
    },
    project_registry::ProjectRootAuthority,
};

use super::{
    AdmittedStandardAcpBinding, NpmPackageManifestRequirement, StandardAcpAdmissionError,
    StandardAcpArtifact, StandardAcpArtifactProvenance, StandardAcpCandidate, admit, profile_for,
};

const OPENCODE_SHIM: &str = "opencode.cmd";
pub(crate) const PACKAGE_BYTES_MAX: u64 = 64 * 1024;
pub(crate) const OPENCODE_BINDING_ID: &str = "opencode-acp";
#[cfg(test)]
pub(crate) const OPENCODE_OBSERVED_VERSION: &str = "1.18.21";

#[derive(Deserialize)]
struct PackageManifest {
    name: String,
    version: String,
    bin: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum OpenCodeAdmissionUnavailable {
    #[error("OpenCode discovery is still in progress")]
    DiscoveryPending,
    #[error("OpenCode is not installed")]
    Absent,
    #[error("the discovered OpenCode installation is invalid")]
    CandidateInvalid,
    #[error("the discovered OpenCode installation changed after admission")]
    Drift,
}

#[derive(Debug)]
enum RegistrationState {
    Pending,
    Unavailable(OpenCodeAdmissionUnavailable),
    Admitted {
        binding: Arc<AdmittedStandardAcpBinding>,
    },
}

#[derive(Debug)]
pub(crate) struct OpenCodeBindingRegistration {
    state: Mutex<RegistrationState>,
}

impl OpenCodeBindingRegistration {
    pub(crate) fn startup() -> Self {
        Self {
            state: Mutex::new(RegistrationState::Pending),
        }
    }

    pub(crate) fn apply_discovery(&self, discovered: Option<&InstalledHarness>) {
        *self.lock_state() = discovered.map_or(
            RegistrationState::Unavailable(OpenCodeAdmissionUnavailable::Absent),
            |discovered| match &discovered.version {
                Some(version) => Self::admit_shim(&discovered.executable, version)
                    .map_or_else(RegistrationState::Unavailable, |binding| {
                        RegistrationState::Admitted { binding }
                    }),
                None => {
                    RegistrationState::Unavailable(OpenCodeAdmissionUnavailable::CandidateInvalid)
                }
            },
        );
    }

    pub(crate) fn refresh(
        &self,
    ) -> Result<Arc<AdmittedStandardAcpBinding>, OpenCodeAdmissionUnavailable> {
        let binding = match &*self.lock_state() {
            RegistrationState::Pending => {
                return Err(OpenCodeAdmissionUnavailable::DiscoveryPending);
            }
            RegistrationState::Unavailable(reason) => return Err(*reason),
            RegistrationState::Admitted { binding } => Arc::clone(binding),
        };
        let refreshed = binding
            .revalidate()
            .map(|()| Arc::clone(&binding))
            .map_err(|_| OpenCodeAdmissionUnavailable::Drift);
        let mut state = self.lock_state();
        match &*state {
            RegistrationState::Admitted { binding: current } if Arc::ptr_eq(current, &binding) => {
                match refreshed {
                    Ok(binding) => {
                        *state = RegistrationState::Admitted {
                            binding: Arc::clone(&binding),
                        };
                        Ok(binding)
                    }
                    Err(reason) => {
                        *state = RegistrationState::Unavailable(reason);
                        Err(reason)
                    }
                }
            }
            RegistrationState::Pending => Err(OpenCodeAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Err(*reason),
            RegistrationState::Admitted { .. } => Err(OpenCodeAdmissionUnavailable::Drift),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), RegistrationState::Admitted { .. })
    }

    pub(crate) fn admitted(&self) -> Option<Arc<AdmittedStandardAcpBinding>> {
        match &*self.lock_state() {
            RegistrationState::Admitted { binding } => Some(Arc::clone(binding)),
            RegistrationState::Pending | RegistrationState::Unavailable(_) => None,
        }
    }

    pub(crate) fn unavailable(&self) -> Option<OpenCodeAdmissionUnavailable> {
        match *self.lock_state() {
            RegistrationState::Pending => Some(OpenCodeAdmissionUnavailable::DiscoveryPending),
            RegistrationState::Unavailable(reason) => Some(reason),
            RegistrationState::Admitted { .. } => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn admitted_for_test(shim: &Path, version: &str) -> Self {
        let binding = Self::admit_shim(shim, version).expect("test OpenCode layout must admit");
        Self {
            state: Mutex::new(RegistrationState::Admitted { binding }),
        }
    }

    fn admit_shim(
        shim: &Path,
        version: &str,
    ) -> Result<Arc<AdmittedStandardAcpBinding>, OpenCodeAdmissionUnavailable> {
        let profile =
            profile_for("opencode").ok_or(OpenCodeAdmissionUnavailable::CandidateInvalid)?;
        let candidate = candidate_from_global_npm_shim(shim, version.to_owned())
            .ok_or(OpenCodeAdmissionUnavailable::CandidateInvalid)?;
        admit(profile, candidate).map_err(|_| OpenCodeAdmissionUnavailable::CandidateInvalid)
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, RegistrationState> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                *state = RegistrationState::Unavailable(OpenCodeAdmissionUnavailable::Drift);
                state
            }
        }
    }
}

pub(crate) struct OpenCodeRuntimeFactory {
    registration: Arc<OpenCodeBindingRegistration>,
    binding_digest: [u8; 32],
    project_root: ProjectRootAuthority,
    mcp: crate::acp::mcp::McpAdmission,
}

impl OpenCodeRuntimeFactory {
    pub(crate) fn from_admitted_binding(
        registration: Arc<OpenCodeBindingRegistration>,
        binding: Arc<AdmittedStandardAcpBinding>,
        project_root: ProjectRootAuthority,
    ) -> Self {
        Self::with_mcp(registration, binding, project_root, Default::default())
    }

    pub(crate) fn with_mcp(
        registration: Arc<OpenCodeBindingRegistration>,
        binding: Arc<AdmittedStandardAcpBinding>,
        project_root: ProjectRootAuthority,
        mcp: crate::acp::mcp::McpAdmission,
    ) -> Self {
        Self {
            registration,
            binding_digest: binding.binding_digest(),
            project_root,
            mcp,
        }
    }
}

impl RuntimeFactory for OpenCodeRuntimeFactory {
    fn construct(
        self: Box<Self>,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<NativeRuntime, AdapterRuntimeError> {
        let Self {
            registration,
            binding_digest,
            project_root,
            mcp,
        } = *self;
        Ok(NativeRuntime::new(
            super::opencode_runtime::OpenCodeRuntime::new(
                registration,
                binding_digest,
                runtime_epoch,
                session_id,
                project_root,
                mcp,
            )?,
        ))
    }
}

pub(crate) fn candidate_from_global_npm_shim(
    shim: &Path,
    reported_version: String,
) -> Option<StandardAcpCandidate> {
    let profile = profile_for("opencode")?;
    if !profile.version_shape.accepts(&reported_version)
        || !authentic_harness_file(shim, OPENCODE_SHIM)
    {
        return None;
    }
    let root = shim.parent()?.to_path_buf();
    let requirement = profile.package_manifest?;
    let package_relative = package_relative(requirement);
    let runtime_relative = runtime_relative(requirement)?;
    let package_manifest =
        package_artifact(&root, &package_relative, requirement, &reported_version)?;
    let runtime = root.join(&runtime_relative);
    authentic_npm_global_descendant(&root, &runtime_relative).then_some(())?;
    Some(StandardAcpCandidate {
        runtime: artifact(&root, &runtime_relative, runtime)?,
        entrypoint: None,
        package_manifest: Some(package_manifest),
        reported_version,
    })
}

pub(crate) fn validate_package_manifest(
    artifact: &StandardAcpArtifact,
    requirement: NpmPackageManifestRequirement,
    reported_version: &str,
) -> Result<(), StandardAcpAdmissionError> {
    let StandardAcpArtifactProvenance::NpmGlobal { root, relative } = &artifact.provenance else {
        return Err(StandardAcpAdmissionError::Manifest);
    };
    if relative != &package_relative(requirement) || artifact.path != root.join(relative) {
        return Err(StandardAcpAdmissionError::Manifest);
    }
    let bytes = bounded_file_bytes(&artifact.path).ok_or(StandardAcpAdmissionError::Manifest)?;
    if u64::try_from(bytes.len()).ok() != Some(artifact.byte_len)
        || digest(&bytes) != artifact.sha256
        || !manifest_matches(&bytes, requirement, reported_version)
    {
        return Err(StandardAcpAdmissionError::Manifest);
    }
    Ok(())
}

fn package_artifact(
    root: &Path,
    relative: &Path,
    requirement: NpmPackageManifestRequirement,
    reported_version: &str,
) -> Option<StandardAcpArtifact> {
    let path = root.join(relative);
    authentic_npm_global_descendant(root, relative).then_some(())?;
    let bytes = bounded_file_bytes(&path)?;
    manifest_matches(&bytes, requirement, reported_version).then_some(())?;
    Some(StandardAcpArtifact {
        path,
        sha256: digest(&bytes),
        byte_len: u64::try_from(bytes.len()).ok()?,
        provenance: StandardAcpArtifactProvenance::NpmGlobal {
            root: root.to_path_buf(),
            relative: relative.to_path_buf(),
        },
    })
}

fn artifact(root: &Path, relative: &Path, path: PathBuf) -> Option<StandardAcpArtifact> {
    let metadata = std::fs::metadata(&path).ok()?;
    Some(StandardAcpArtifact {
        sha256: sha256_file(&path)?,
        byte_len: metadata.len(),
        path,
        provenance: StandardAcpArtifactProvenance::NpmGlobal {
            root: root.to_path_buf(),
            relative: relative.to_path_buf(),
        },
    })
}

fn package_relative(requirement: NpmPackageManifestRequirement) -> PathBuf {
    PathBuf::from("node_modules")
        .join(requirement.package_dir)
        .join("package.json")
}

pub(super) fn runtime_relative(requirement: NpmPackageManifestRequirement) -> Option<PathBuf> {
    Path::new(requirement.bin_path)
        .strip_prefix("./")
        .ok()
        .map(|path| {
            PathBuf::from("node_modules")
                .join(requirement.package_dir)
                .join(path)
        })
}

fn bounded_file_bytes(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(PACKAGE_BYTES_MAX.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    (u64::try_from(bytes.len()).ok()? <= PACKAGE_BYTES_MAX).then_some(bytes)
}

fn manifest_matches(
    bytes: &[u8],
    requirement: NpmPackageManifestRequirement,
    reported_version: &str,
) -> bool {
    let Ok(manifest) = serde_json::from_slice::<PackageManifest>(bytes) else {
        return false;
    };
    manifest.name == requirement.name
        && manifest.version == reported_version
        && manifest
            .bin
            .get(requirement.bin_name)
            .is_some_and(|path| path == requirement.bin_path)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        bindings::{
            adapter::{
                AdapterCommand, AdapterEvent, AdapterRuntimeError, EgressResolution,
                HarnessAdapter, ResourceMapper,
            },
            runtime::RuntimeFactory,
        },
        contract::{
            ProjectGeneration, RequiredText, RuntimeEpoch, SageAgentSessionId, SageProjectId,
            SageRootId,
        },
        intent::{CapabilitySnapshotV1, NormalizedPayload, Prompt},
        lifecycle::TurnState,
        transport::{Frame, IncrementalFrameDecoder},
    };
    use serde_json::Value;
    use std::path::PathBuf;

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Mapper;

    impl ResourceMapper for Mapper {
        fn map_existing(
            &self,
            _: &str,
        ) -> Result<
            crate::acp::intent::SafeResourceRef,
            crate::acp::intent::UnavailableResourceReason,
        > {
            Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
        }
    }

    fn frame(value: Value) -> Frame {
        let mut bytes = serde_json::to_vec(&value).expect("json");
        bytes.push(b'\n');
        let mut decoder = IncrementalFrameDecoder::new();
        let mut complete = None;
        for chunk in bytes.chunks(8 * 1024) {
            complete = decoder.push_chunk(chunk).expect("valid frame").or(complete);
        }
        complete.expect("complete frame")
    }

    fn egress_value(egress: &crate::acp::bindings::adapter::AdapterEgress) -> Value {
        serde_json::from_slice(egress.frame().payload()).expect("egress json")
    }

    fn scoped_permission_id(label: &str, epoch: u64) -> crate::acp::contract::SageServerRequestId {
        let session = SageAgentSessionId::parse(format!("session-{label}")).expect("session");
        let scoped = super::super::opencode_runtime::scoped_permission_id(
            &session,
            RuntimeEpoch::new(epoch).expect("epoch"),
            "number:0",
        )
        .expect("scoped request");
        crate::acp::contract::SageServerRequestId::parse(scoped.as_str()).expect("durable request")
    }

    fn curated_registry_snapshot(root: &TempRoot, version: &str) -> CapabilitySnapshotV1 {
        let shim = root.0.join("npm").join(OPENCODE_SHIM);
        let registry = crate::acp::bindings::registry::BindingRegistry::new_with_opencode_for_test(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
            crate::acp::bindings::claude::ClaudeBindingRegistration::absent_for_test(),
            OpenCodeBindingRegistration::admitted_for_test(&shim, version),
        );
        let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
        let snapshot = admitted
            .supported_capabilities()
            .expect("curated OpenCode snapshot");
        drop(admitted);
        snapshot
    }

    fn ready_runtime(
        root: &TempRoot,
        epoch: u64,
        label: &str,
        version: &str,
    ) -> crate::acp::bindings::runtime::NativeRuntime {
        ready_runtime_with_mode(root, epoch, label, version, true, "build")
    }

    fn switching_runtime(
        root: &TempRoot,
        epoch: u64,
        label: &str,
        version: &str,
    ) -> crate::acp::bindings::runtime::NativeRuntime {
        ready_runtime_with_mode(root, epoch, label, version, true, "plan")
    }

    fn ready_runtime_with_mode(
        root: &TempRoot,
        epoch: u64,
        label: &str,
        version: &str,
        advertise_build: bool,
        current_mode: &str,
    ) -> crate::acp::bindings::runtime::NativeRuntime {
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse(format!("project-{label}")).expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse(format!("root-{label}")).expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(root, version, project_root))
            .construct(
                RuntimeEpoch::new(epoch).expect("epoch"),
                SageAgentSessionId::parse(format!("session-{label}")).expect("session"),
            )
            .expect("construct runtime");
        let spec = runtime.launch_spec().expect("launch spec");
        let config = spec
            .environment
            .as_slice()
            .iter()
            .find(|(key, _)| key == "OPENCODE_CONFIG_CONTENT")
            .and_then(|(_, value)| value.to_str())
            .expect("policy config");
        let config: Value = serde_json::from_str(config).expect("policy JSON");
        assert_eq!(config["permission"]["*"], "ask");
        assert_eq!(config["permission"]["read"], "allow");
        let mut inbound = inbound_smoke_frames().into_iter();
        let initialize = runtime
            .command(AdapterCommand::Initialize)
            .expect("initialize command");
        runtime
            .resolve_egress(initialize.into_token(), EgressResolution::Written)
            .expect("initialize written");
        runtime
            .ingest(
                &frame(inbound.next().expect("initialize response")),
                &Mapper,
            )
            .expect("initialize response");
        let session = runtime
            .command(AdapterCommand::NewSession {
                native_root: r"C:\fixture-workspace\opencode\smoke".into(),
                model: None,
            })
            .expect("new session");
        runtime
            .resolve_egress(session.into_token(), EgressResolution::Written)
            .expect("session written");
        let mut session_response = inbound.next().expect("session response");
        let mut options = vec![serde_json::json!({"value":"plan","name":"Plan"})];
        if advertise_build {
            options.push(serde_json::json!({"value":"build","name":"Build"}));
        }
        session_response["result"]["configOptions"] = serde_json::json!([{
            "id": "mode", "name": "Session Mode", "category": "mode", "type": "select",
            "currentValue": current_mode,
            "options": options
        }]);
        runtime
            .ingest(&frame(session_response), &Mapper)
            .expect("session response");
        runtime
    }

    fn inbound_smoke_frames() -> Vec<Value> {
        include_str!("../../../../tests/fixtures/harness-traces/opencode/smoke/frames.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("fixture record"))
            .filter(|record| record["direction"] == "in")
            .map(|record| record["frame"].clone())
            .collect()
    }

    fn admitted_factory(
        root: &TempRoot,
        version: &str,
        project_root: ProjectRootAuthority,
    ) -> OpenCodeRuntimeFactory {
        let npm = root.0.join("npm");
        let runtime = npm
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        std::fs::create_dir_all(runtime.parent().expect("runtime parent")).expect("create layout");
        std::fs::write(&runtime, b"synthetic-opencode").expect("write runtime");
        std::fs::write(
            npm.join("node_modules").join("opencode-ai").join("package.json"),
            format!(
                r#"{{"name":"opencode-ai","version":"{version}","bin":{{"opencode":"./bin/opencode.exe"}}}}"#
            ),
        )
        .expect("write manifest");
        let shim = npm.join(OPENCODE_SHIM);
        std::fs::write(&shim, b"synthetic-shim").expect("write shim");
        let registration = Arc::new(OpenCodeBindingRegistration::admitted_for_test(
            &shim, version,
        ));
        let binding = registration.admitted().expect("admitted binding");
        OpenCodeRuntimeFactory::from_admitted_binding(registration, binding, project_root)
    }

    #[test]
    fn factory_runtime_accepts_the_observed_initialize_command() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-runtime-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-runtime").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-runtime").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(
            &root,
            OPENCODE_OBSERVED_VERSION,
            project_root,
        ))
        .construct(
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-runtime").expect("session"),
        )
        .expect("construct runtime");

        assert!(runtime.command(AdapterCommand::Initialize).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn opencode_launch_spec_carries_the_windows_process_baseline_environment() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-env-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-env").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-env").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(
            &root,
            OPENCODE_OBSERVED_VERSION,
            project_root,
        ))
        .construct(
            RuntimeEpoch::new(11).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-env").expect("session"),
        )
        .expect("construct runtime");

        let spec = runtime.launch_spec().expect("launch spec");
        let system_root = std::env::var_os("SystemRoot").expect("SystemRoot");
        assert!(
            spec.environment
                .as_slice()
                .iter()
                .any(|(key, value)| key == "SystemRoot" && value == &system_root),
            "an OpenCode launch spec without SystemRoot makes the native child fail fast"
        );
    }

    #[cfg(windows)]
    #[test]
    fn opencode_launch_spec_forces_a_client_permission_round_trip() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-policy-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-policy").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-policy").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(
            &root,
            OPENCODE_OBSERVED_VERSION,
            project_root,
        ))
        .construct(
            RuntimeEpoch::new(12).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-policy").expect("session"),
        )
        .expect("construct runtime");

        let spec = runtime.launch_spec().expect("launch spec");
        let config = spec
            .environment
            .as_slice()
            .iter()
            .find(|(key, _)| key == "OPENCODE_CONFIG_CONTENT")
            .map(|(_, value)| value.to_str().expect("UTF-8 config"))
            .expect("Sage policy config");
        let value: serde_json::Value = serde_json::from_str(config).expect("config JSON");
        assert!(value.get("agent").is_none());
        assert_eq!(value["permission"]["*"], "ask");
        assert_eq!(value["permission"]["read"], "allow");
    }

    #[test]
    fn factory_runtime_replays_the_synthetic_open_code_smoke_turn() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-runtime-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-smoke").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-smoke").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(
            &root,
            OPENCODE_OBSERVED_VERSION,
            project_root,
        ))
        .construct(
            RuntimeEpoch::new(2).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-smoke").expect("session"),
        )
        .expect("construct runtime");
        let spec = runtime.launch_spec().expect("launch spec");
        let config = spec
            .environment
            .as_slice()
            .iter()
            .find(|(key, _)| key == "OPENCODE_CONFIG_CONTENT")
            .and_then(|(_, value)| value.to_str())
            .expect("policy config");
        let config: Value = serde_json::from_str(config).expect("policy JSON");
        assert_eq!(config["permission"]["*"], "ask");
        let mut inbound = inbound_smoke_frames().into_iter();

        let initialize = runtime
            .command(AdapterCommand::Initialize)
            .expect("initialize command");
        runtime
            .resolve_egress(initialize.into_token(), EgressResolution::Written)
            .expect("initialize written");
        let initialized = runtime
            .ingest(
                &frame(inbound.next().expect("initialize response")),
                &Mapper,
            )
            .expect("initialize response");
        assert!(matches!(
            initialized.events(),
            [AdapterEvent::Initialized { .. }]
        ));
        assert!(initialized.egress().is_empty());
        let invalid_response = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32601, "message": "Method not found"}
                })),
                &Mapper,
            )
            .err()
            .expect("null-id method rejection must fail closed");
        assert_eq!(invalid_response, AdapterRuntimeError::Frame);
        let capabilities = runtime.capabilities().expect("capabilities");
        assert!(capabilities.new_session && capabilities.prompt);
        assert!(capabilities.cancel && capabilities.close_session);
        assert!(!capabilities.resume_session && !capabilities.load_session);

        let session = runtime
            .command(AdapterCommand::NewSession {
                native_root: r"C:\fixture-workspace\opencode\smoke".into(),
                model: None,
            })
            .expect("new session");
        runtime
            .resolve_egress(session.into_token(), EgressResolution::Written)
            .expect("session written");
        let mut session_response = inbound.next().expect("session response");
        session_response["result"]["configOptions"] = serde_json::json!([{
            "id": "mode", "name": "Session Mode", "category": "mode", "type": "select",
            "currentValue": "build",
            "options": [{"value":"build","name":"Build"}]
        }]);
        let ready = runtime
            .ingest(&frame(session_response), &Mapper)
            .expect("session response");
        assert!(matches!(
            ready.events(),
            [AdapterEvent::SessionReady { loaded: false, .. }]
        ));

        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "synthetic prompt").expect("prompt"),
                client_message_id: RequiredText::new("client", "synthetic-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        let mut events = Vec::new();
        for value in inbound {
            let batch = runtime.ingest(&frame(value), &Mapper).expect("smoke frame");
            events.extend_from_slice(batch.events());
        }

        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Running,
                ..
            }
        )));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    AdapterEvent::Item {
                        payload: NormalizedPayload::ThoughtSummary { .. },
                        ..
                    }
                ))
                .count(),
            3
        );
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                payload: NormalizedPayload::Text { text },
                ..
            } if text.as_str() == "synthetic final"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Usage {
                payload: NormalizedPayload::Usage {
                    input_tokens: 5,
                    output_tokens: 1,
                    context_window: Some(4096),
                },
                ..
            }
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        )));
    }

    #[test]
    fn an_uncharacterised_admitted_build_still_attempts_the_mandatory_session_core() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-uncharacterised-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-uncharacterised").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-uncharacterised").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(&root, "1.18.22", project_root))
            .construct(
                RuntimeEpoch::new(3).expect("epoch"),
                SageAgentSessionId::parse("session-opencode-uncharacterised").expect("session"),
            )
            .expect("construct runtime");
        let initialize = runtime
            .command(AdapterCommand::Initialize)
            .expect("initialize command");
        runtime
            .resolve_egress(initialize.into_token(), EgressResolution::Written)
            .expect("initialize written");
        runtime
            .ingest(
                &frame(
                    inbound_smoke_frames()
                        .into_iter()
                        .next()
                        .expect("initialize response"),
                ),
                &Mapper,
            )
            .expect("initialize response");

        let capabilities = runtime.capabilities().expect("capabilities");
        assert!(capabilities.new_session && capabilities.prompt);
        assert!(capabilities.cancel && capabilities.close_session);
        assert!(!capabilities.resume_session && !capabilities.load_session);
        assert!(!capabilities.native_session_fork && !capabilities.active_turn_steer);
        assert_eq!(capabilities.model_count, 0);
        assert_eq!(capabilities.config_count, 0);
        assert!(
            runtime
                .command(AdapterCommand::NewSession {
                    native_root: r"C:\fixture-workspace\opencode\unknown".into(),
                    model: None,
                })
                .is_ok()
        );
    }

    #[test]
    fn new_session_auth_refusal_is_not_reported_as_a_protocol_error() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-sign-in-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-sign-in").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-sign-in").expect("root"),
            &root.0,
        )
        .expect("project root");
        let mut runtime = Box::new(admitted_factory(
            &root,
            OPENCODE_OBSERVED_VERSION,
            project_root,
        ))
        .construct(
            RuntimeEpoch::new(4).expect("epoch"),
            SageAgentSessionId::parse("session-opencode-sign-in").expect("session"),
        )
        .expect("construct runtime");
        let initialize = runtime
            .command(AdapterCommand::Initialize)
            .expect("initialize command");
        runtime
            .resolve_egress(initialize.into_token(), EgressResolution::Written)
            .expect("initialize written");
        runtime
            .ingest(
                &frame(
                    inbound_smoke_frames()
                        .into_iter()
                        .next()
                        .expect("initialize response"),
                ),
                &Mapper,
            )
            .expect("initialize response");
        let new_session = runtime
            .command(AdapterCommand::NewSession {
                native_root: r"C:\fixture-workspace\opencode\auth".into(),
                model: None,
            })
            .expect("new session command");
        runtime
            .resolve_egress(new_session.into_token(), EgressResolution::Written)
            .expect("new session written");

        assert!(matches!(
            runtime.ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "error": {"code": -32000, "message": "authentication required"}
                })),
                &Mapper,
            ),
            Err(AdapterRuntimeError::SignInRequired)
        ));
    }

    #[test]
    fn an_admitted_opencode_turn_cancels_through_the_standard_acp_notification() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-cancel-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 5, "opencode-cancel", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "synthetic prompt").expect("prompt"),
                client_message_id: RequiredText::new("client", "cancel-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");

        let cancel = runtime
            .command(AdapterCommand::Cancel)
            .expect("cancel command");
        assert!(cancel.token().is_notification());
        let value = egress_value(&cancel);
        assert_eq!(value["method"], "session/cancel");
        assert_eq!(value["params"]["sessionId"], "synthetic-session");
        assert!(value.get("id").is_none());
        runtime
            .resolve_egress(cancel.into_token(), EgressResolution::Written)
            .expect("cancel written");
        let settled = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "result": {"stopReason": "cancelled"}
                })),
                &Mapper,
            )
            .expect("cancelled prompt result");

        assert!(settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn read_only_mode_is_acknowledged_before_opencode_receives_the_prompt() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-read-only-mode-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = switching_runtime(
            &root,
            13,
            "opencode-read-only-mode",
            OPENCODE_OBSERVED_VERSION,
        );
        let select = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read the project").expect("prompt"),
                client_message_id: RequiredText::new("client", "read-only-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("mode request");
        let request = egress_value(&select);
        assert_eq!(request["method"], "session/set_mode");
        assert_eq!(request["params"]["modeId"], "build");
        runtime
            .resolve_egress(select.into_token(), EgressResolution::Written)
            .expect("mode written");
        let selected = runtime
            .ingest(
                &frame(serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}})),
                &Mapper,
            )
            .expect("mode acknowledged");
        assert_eq!(selected.egress().len(), 1);
        assert_eq!(
            egress_value(&selected.egress()[0])["method"],
            "session/prompt"
        );
        let prompt_token = selected.egress()[0].token().clone();
        runtime
            .resolve_egress(prompt_token, EgressResolution::Written)
            .expect("prompt written");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":4,"result":{"stopReason":"end_turn"}
                })),
                &Mapper,
            )
            .expect("prompt settled");
        let second = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read again").expect("prompt"),
                client_message_id: RequiredText::new("client", "second-read-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("same-mode prompt");
        assert_eq!(egress_value(&second)["method"], "session/prompt");
        runtime
            .resolve_egress(second.into_token(), EgressResolution::Written)
            .expect("second prompt written");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":5,"result":{"stopReason":"end_turn"}
                })),
                &Mapper,
            )
            .expect("second prompt settled");
        let write_mode = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "edit a file").expect("prompt"),
                client_message_id: RequiredText::new("client", "write-mode-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("write-mode selection");
        assert_eq!(egress_value(&write_mode)["method"], "session/prompt");
    }

    #[test]
    fn read_only_opencode_declines_the_real_binary_write_permission_request() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-read-only-permission-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime =
            ready_runtime(&root, 34, "opencode-permission", OPENCODE_OBSERVED_VERSION);
        let first = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Create x2.txt").expect("prompt"),
                client_message_id: RequiredText::new("client", "readonly-write").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("prompt command");
        assert_eq!(egress_value(&first)["method"], "session/prompt");
        runtime
            .resolve_egress(first.token().clone(), EgressResolution::Written)
            .expect("prompt written");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call","toolCallId":"call_function_m2pmbkrdd3p8_1",
                            "title":"write","kind":"edit","status":"pending","locations":[],"rawInput":{}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("tool call");
        let permission = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                        "sessionId":"synthetic-session",
                        "toolCall":{"toolCallId":"call_function_m2pmbkrdd3p8_1","title":"x2.txt","kind":"edit","status":"pending","locations":[],"rawInput":{}},
                        "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},{"optionId":"reject","kind":"reject_once","name":"Reject"}]
                    }
                })),
                &Mapper,
            )
            .expect("read-only request must be declined, not tear down");
        assert_eq!(permission.egress().len(), 1);
        assert_eq!(egress_value(&permission.egress()[0])["id"], 0);
        assert_eq!(
            egress_value(&permission.egress()[0])["result"]["outcome"]["outcome"],
            "cancelled"
        );
        let token = permission.egress()[0].token().clone();
        runtime
            .commit_mandatory_egress(&token)
            .expect("decline committed before write");
        runtime
            .resolve_egress(token, EgressResolution::Queued)
            .expect("decline queued");
        runtime
            .resolve_egress(
                permission.egress()[0].token().clone(),
                EgressResolution::Written,
            )
            .expect("decline written");
        let settled = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call_update","toolCallId":"call_function_m2pmbkrdd3p8_1",
                            "title":"write","kind":"edit","status":"failed","rawOutput":{"error":"The user rejected permission to use this specific tool call."}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("rejected tool update");
        assert!(settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                tool_state: Some(crate::acp::lifecycle::ToolState::Declined),
                ..
            }
        )));
    }

    #[test]
    fn workspace_write_asks_before_an_edit_outside_the_project() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-workspace-outside-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(
            &root,
            36,
            "opencode-workspace-outside",
            OPENCODE_OBSERVED_VERSION,
        );
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "edit a file").expect("prompt"),
                client_message_id: RequiredText::new("client", "workspace-outside")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"outside-edit",
                    "title":"write","kind":"edit","status":"pending","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("tool call");
        let outside = root.0.parent().expect("parent").join("outside.txt");
        let path = outside.to_str().expect("path");
        let permission = runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                "sessionId":"synthetic-session",
                "toolCall":{"toolCallId":"outside-edit","title":path,"kind":"edit",
                    "status":"pending","locations":[{"path":path}],"rawInput":{"filepath":path}},
                "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},
                    {"optionId":"reject","kind":"reject_once","name":"Reject"}]
            }
        })), &Mapper).expect("permission request");
        assert!(
            permission.egress().is_empty(),
            "outside edit must not auto-allow"
        );
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
        let offered = permission
            .events()
            .iter()
            .find_map(|event| match event {
                AdapterEvent::PermissionRequest { payload, .. } => {
                    Some(serde_json::to_value(payload).expect("payload"))
                }
                _ => None,
            })
            .expect("OpenCode permission payload");
        assert_eq!(
            offered["payload"]["offeredOptionKinds"],
            serde_json::json!(["allow_once", "reject_once"])
        );
        let decision = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("opencode-workspace-outside", 36),
                decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
            })
            .expect("user denial");
        assert_eq!(
            egress_value(&decision)["result"]["outcome"]["outcome"],
            "cancelled"
        );
    }

    fn workspace_permission_case(
        root: &TempRoot,
        label: &str,
        kind: &str,
        target: Option<&str>,
        access_mode: crate::settings_store::SessionAccessMode,
    ) -> crate::acp::bindings::adapter::AdapterBatch {
        let mut runtime = ready_runtime(root, 37, label, OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "run a tool").expect("prompt"),
                client_message_id: RequiredText::new("client", label).expect("client"),
                access_mode,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"workspace-tool",
                    "title":"tool","kind":kind,"status":"pending","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("tool call");
        let locations = target.map_or(
            serde_json::json!([]),
            |path| serde_json::json!([{"path":path}]),
        );
        let raw_input = target.map_or(
            serde_json::json!({"command":"pwd"}),
            |path| serde_json::json!({"filepath":path}),
        );
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                        "sessionId":"synthetic-session",
                        "toolCall":{"toolCallId":"workspace-tool","title":"tool","kind":kind,
                            "status":"pending","locations":locations,"rawInput":raw_input},
                        "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},
                            {"optionId":"reject","kind":"reject_once","name":"Reject"}]
                    }
                })),
                &Mapper,
            )
            .expect("permission request")
    }

    #[test]
    fn numeric_permission_ids_are_distinct_across_opencode_sessions() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-request-scope-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let request_id = |label| {
            workspace_permission_case(
                &root,
                label,
                "execute",
                None,
                crate::settings_store::SessionAccessMode::WorkspaceWrite,
            )
            .events()
            .iter()
            .find_map(|event| match event {
                AdapterEvent::PermissionRequest {
                    external_request_id,
                    ..
                } => Some(external_request_id.as_str().to_owned()),
                _ => None,
            })
            .expect("permission card")
        };
        assert_ne!(request_id("session-one"), request_id("session-two"));
    }

    #[test]
    fn maximum_length_wire_permission_id_has_bounded_durable_identity() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-long-request-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 52, "long-request", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "run a tool").expect("prompt"),
                client_message_id: RequiredText::new("client", "long-request").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::Ask,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call","toolCallId":"long-tool",
                            "title":"run","kind":"execute","status":"pending",
                            "locations":[],"rawInput":{}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("tool call");
        let permission = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":"x".repeat(512),"method":"session/request_permission",
                    "params":{"sessionId":"synthetic-session","toolCall":{
                        "toolCallId":"long-tool","title":"run","kind":"execute","status":"pending",
                        "locations":[],"rawInput":{"command":"pwd"}
                    },"options":[{"optionId":"once","kind":"allow_once","name":"Allow once"}]}
                })),
                &Mapper,
            )
            .expect("bounded permission");
        let request = permission
            .events()
            .iter()
            .find_map(|event| match event {
                AdapterEvent::PermissionRequest {
                    external_request_id,
                    ..
                } => Some(external_request_id),
                _ => None,
            })
            .expect("permission request");
        assert!(request.as_str().len() <= 512);
    }

    #[test]
    fn workspace_write_auto_allows_an_inside_edit_once() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-inside-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let path = root.0.join("inside.txt");
        assert!(!path.exists(), "this is a new-file request");
        let permission = workspace_permission_case(
            &root,
            "workspace-inside",
            "edit",
            Some(path.to_str().expect("path")),
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
        );
        assert!(
            permission
                .events()
                .iter()
                .all(|event| !matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
        assert_eq!(permission.egress().len(), 1);
        assert_eq!(
            egress_value(&permission.egress()[0])["result"]["outcome"]["outcome"],
            "selected"
        );
        assert_eq!(
            egress_value(&permission.egress()[0])["result"]["outcome"]["optionId"],
            "once"
        );
    }

    #[test]
    fn workspace_write_never_approves_an_edit_for_a_foreign_session() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-foreign-session-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 40, "workspace-foreign", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "edit").expect("prompt"),
                client_message_id: RequiredText::new("client", "workspace-foreign")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"foreign-tool",
                    "title":"write","kind":"edit","status":"pending","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("tool call");
        let path = root.0.join("inside.txt");
        let path = path.to_str().expect("path");
        let result = runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                "sessionId":"foreign-session",
                "toolCall":{"toolCallId":"foreign-tool","title":path,"kind":"edit",
                    "status":"pending","locations":[{"path":path}],"rawInput":{"filepath":path}},
                "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"}]
            }
        })), &Mapper);
        assert!(
            result.is_err(),
            "foreign session must never receive automatic approval"
        );
    }

    #[test]
    fn workspace_write_asks_for_parent_traversal() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-traversal-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let path = root.0.join("..").join("outside.txt");
        let permission = workspace_permission_case(
            &root,
            "workspace-traversal",
            "edit",
            Some(path.to_str().expect("path")),
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[test]
    fn workspace_write_asks_for_an_ads_or_device_filename() {
        for filename in ["safe.txt:alternate", "CON.txt"] {
            let root = TempRoot(std::env::temp_dir().join(format!(
                "sage-opencode-unsafe-name-{}",
                uuid::Uuid::new_v4()
            )));
            std::fs::create_dir_all(&root.0).expect("create root");
            let path = root.0.join(filename);
            let permission = workspace_permission_case(
                &root,
                "workspace-unsafe-name",
                "edit",
                Some(path.to_str().expect("path")),
                crate::settings_store::SessionAccessMode::WorkspaceWrite,
            );
            assert!(permission.egress().is_empty(), "{filename} needs a card");
            assert!(
                permission
                    .events()
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
            );
        }
    }

    #[test]
    fn workspace_write_asks_when_an_edit_request_has_no_target_path() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-no-target-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let permission = workspace_permission_case(
            &root,
            "workspace-no-target",
            "edit",
            None,
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[test]
    fn workspace_write_asks_when_an_inside_file_is_hard_linked_outside() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-hardlink-root-{}",
            uuid::Uuid::new_v4()
        )));
        let outside = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-hardlink-outside-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        std::fs::create_dir_all(&outside.0).expect("create outside");
        let external = outside.0.join("linked.txt");
        std::fs::write(&external, b"original").expect("create external file");
        let path = root.0.join("linked.txt");
        std::fs::hard_link(&external, &path).expect("create hard link");
        let permission = workspace_permission_case(
            &root,
            "workspace-hardlink",
            "edit",
            Some(path.to_str().expect("path")),
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_write_asks_when_an_inside_junction_points_outside() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-junction-root-{}",
            uuid::Uuid::new_v4()
        )));
        let outside = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-junction-outside-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        std::fs::create_dir_all(&outside.0).expect("create outside");
        let link = root.0.join("alias");
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside.0)
            .status()
            .expect("create junction command");
        assert!(status.success(), "create junction");
        let path = link.join("outside.txt");
        let permission = workspace_permission_case(
            &root,
            "workspace-junction",
            "edit",
            Some(path.to_str().expect("path")),
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[test]
    fn workspace_write_asks_for_non_edit_tools() {
        for kind in ["execute", "fetch", "task", "other"] {
            let root = TempRoot(
                std::env::temp_dir().join(format!("sage-opencode-{kind}-{}", uuid::Uuid::new_v4())),
            );
            std::fs::create_dir_all(&root.0).expect("create root");
            let permission = workspace_permission_case(
                &root,
                &format!("workspace-{kind}"),
                kind,
                None,
                crate::settings_store::SessionAccessMode::WorkspaceWrite,
            );
            assert!(permission.egress().is_empty(), "{kind} must not auto-allow");
            assert!(
                permission
                    .events()
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. })),
                "{kind} must show a card"
            );
        }
    }

    #[test]
    fn ask_mode_sends_an_edit_to_the_user_permission_card() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-edit-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let path = root.0.join("ask-edit.txt");
        let permission = workspace_permission_case(
            &root,
            "ask-edit",
            "edit",
            Some(path.to_str().expect("path")),
            crate::settings_store::SessionAccessMode::Ask,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    fn ask_edit_runtime(
        root: &TempRoot,
        label: &str,
    ) -> crate::acp::bindings::runtime::NativeRuntime {
        let mut runtime = ready_runtime(root, 38, label, OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "edit a file").expect("prompt"),
                client_message_id: RequiredText::new("client", label).expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::Ask,
            })
            .expect("ask prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"ask-tool",
                    "title":"write","kind":"edit","status":"pending","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("tool call");
        let path = root.0.join("ask-target.txt");
        let path = path.to_str().expect("path");
        let permission = runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                "sessionId":"synthetic-session",
                "toolCall":{"toolCallId":"ask-tool","title":path,"kind":"edit",
                    "status":"pending","locations":[{"path":path}],"rawInput":{"filepath":path}},
                "options":[{"optionId":"once","kind":"allow_once","name":"Allow once"},
                    {"optionId":"always","kind":"allow_always","name":"Always allow"},
                    {"optionId":"reject","kind":"reject_once","name":"Reject"}]
            }
        })), &Mapper).expect("permission request");
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
        runtime
    }

    #[test]
    fn ask_mode_approval_sends_allow_once_and_accepts_the_tool_completion() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-allow-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "ask-allow");
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("ask-allow", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approve");
        assert_eq!(
            egress_value(&selected)["result"]["outcome"]["outcome"],
            "selected"
        );
        assert_eq!(
            egress_value(&selected)["result"]["outcome"]["optionId"],
            "once"
        );
        runtime
            .resolve_egress(selected.into_token(), EgressResolution::Written)
            .expect("approval written");
        let path = root.0.join("ask-target.txt");
        let path = path.to_str().expect("path");
        let completed = runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call_update","toolCallId":"ask-tool",
                    "title":"ask-target.txt","status":"completed",
                    "content":[{"type":"content","content":{"type":"text","text":"Wrote file successfully."}}],
                    "rawOutput":{"output":"Wrote file successfully.","metadata":{
                        "diagnostics":{},"filepath":path,"exists":false,"truncated":false
                    }}
                }
            }
        })), &Mapper).expect("tool completion");
        assert!(completed.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Item {
                tool_state: Some(crate::acp::lifecycle::ToolState::Completed),
                ..
            }
        )));
    }

    #[test]
    fn ask_mode_always_allow_selects_the_native_opencode_option() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-always-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "ask-always");
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("ask-always", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowAlways,
            })
            .expect("native always approval");
        assert_eq!(
            egress_value(&selected)["result"]["outcome"]["optionId"],
            "always"
        );
    }

    #[test]
    fn approved_real_opencode_client_write_is_bounded_to_its_permission_target() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-client-write-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "client-write");
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("client-write", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approve");
        runtime
            .resolve_egress(selected.into_token(), EgressResolution::Written)
            .expect("approval written");
        let path = root.0.join("ask-target.txt");
        let result = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":2,"method":"fs/write_text_file","params":{
                        "sessionId":"synthetic-session",
                        "path":path.to_str().expect("path"),
                        "content":"RAW APPROVED PROBE"
                    }
                })),
                &Mapper,
            )
            .expect("approved client write");
        assert_eq!(egress_value(&result.egress()[0])["id"], 2);
        assert_eq!(
            egress_value(&result.egress()[0])["result"],
            serde_json::json!({})
        );
        let token = result.egress()[0].token().clone();
        runtime
            .resolve_egress(token.clone(), EgressResolution::Queued)
            .expect("response queued");
        runtime
            .resolve_egress(token, EgressResolution::Written)
            .expect("response written");
        assert_eq!(
            std::fs::read_to_string(path).expect("written file"),
            "RAW APPROVED PROBE"
        );
    }

    #[test]
    fn captured_opencode_allow_and_client_write_frame_shape_round_trips() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-captured-fs-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 51, "captured-fs", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "write fixture file").expect("prompt"),
                client_message_id: RequiredText::new("client", "captured-fs").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::Ask,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call","toolCallId":"fixture-tool",
                            "title":"write","kind":"edit","status":"pending",
                            "locations":[],"rawInput":{}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("tool call");
        let target = root.0.join("captured.txt");
        let path = target.to_str().expect("target path");
        let escaped = serde_json::to_string(path).expect("escaped path");
        let raw = include_str!("testdata/opencode_real_allow_write_20260929.json")
            .replace("__TARGET__", &escaped[1..escaped.len() - 1]);
        let fixture: Value = serde_json::from_str(&raw).expect("fixture json");
        let permission = runtime
            .ingest(&frame(fixture["permission"].clone()), &Mapper)
            .expect("captured permission");
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("captured-fs", 51),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approve");
        runtime
            .resolve_egress(selected.into_token(), EgressResolution::Written)
            .expect("approval written");
        let written = runtime
            .ingest(&frame(fixture["clientWrite"].clone()), &Mapper)
            .expect("captured client write");
        assert_eq!(
            egress_value(&written.egress()[0])["result"],
            serde_json::json!({})
        );
        assert_eq!(
            std::fs::read_to_string(target).expect("target"),
            "RAW APPROVED PROBE"
        );
    }

    #[test]
    fn client_write_refuses_wrong_session_target_and_duplicate_without_teardown() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-client-write-refusal-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "client-write-refusal");
        let target = root.0.join("ask-target.txt");
        let other = root.0.join("other.txt");
        for (id, session, path) in [
            (3, "wrong-session", target.as_path()),
            (4, "synthetic-session", other.as_path()),
            (5, "synthetic-session", target.as_path()),
        ] {
            let response = runtime
                .ingest(
                    &frame(serde_json::json!({
                        "jsonrpc":"2.0","id":id,"method":"fs/write_text_file","params":{
                            "sessionId":session,"path":path.to_str().expect("path"),"content":"NO"
                        }
                    })),
                    &Mapper,
                )
                .expect("refusal response");
            assert!(egress_value(&response.egress()[0])["error"].is_object());
            assert!(response.events().iter().any(|event| matches!(
                event,
                AdapterEvent::Error(crate::acp::intent::NormalizedPayload::Error { .. })
            )));
            assert!(!path.exists());
        }
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("client-write-refusal", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approve");
        runtime
            .resolve_egress(selected.into_token(), EgressResolution::Written)
            .expect("approval written");
        let oversized = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":10,"method":"fs/write_text_file","params":{
                        "sessionId":"synthetic-session", "path":target.to_str().expect("path"),
                        "content":"X".repeat(256 * 1024 + 1)
                    }
                })),
                &Mapper,
            )
            .expect("oversized response");
        assert_eq!(
            egress_value(&oversized.egress()[0])["error"]["code"],
            -32602
        );
        assert!(!target.exists());
        for (id, expected_success) in [(6, true), (7, false)] {
            let response = runtime
                .ingest(
                    &frame(serde_json::json!({
                        "jsonrpc":"2.0","id":id,"method":"fs/write_text_file","params":{
                            "sessionId":"synthetic-session", "path":target.to_str().expect("path"),
                            "content":if expected_success { "YES" } else { "REPLAY" }
                        }
                    })),
                    &Mapper,
                )
                .expect("client response");
            assert_eq!(
                egress_value(&response.egress()[0])["result"].is_object(),
                expected_success
            );
        }
        assert_eq!(std::fs::read_to_string(target).expect("target"), "YES");
        assert!(!other.exists());
    }

    #[test]
    fn client_fs_read_and_unknown_methods_receive_errors() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-client-fs-unknown-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "client-fs-unknown");
        for (id, method, code) in [(8, "fs/read_text_file", -32000), (9, "fs/other", -32601)] {
            let response = runtime
                .ingest(
                    &frame(serde_json::json!({
                        "jsonrpc":"2.0","id":id,"method":method,"params":{}
                    })),
                    &Mapper,
                )
                .expect("method response");
            assert_eq!(egress_value(&response.egress()[0])["error"]["code"], code);
        }
    }

    #[test]
    fn client_write_refuses_when_another_tool_is_pending() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-client-write-concurrent-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "client-write-concurrent");
        runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call","toolCallId":"other-tool",
                            "title":"another edit","kind":"edit","status":"pending",
                            "locations":[],"rawInput":{}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("other tool");
        let selected = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("client-write-concurrent", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
            })
            .expect("approval");
        runtime
            .resolve_egress(selected.into_token(), EgressResolution::Written)
            .expect("approval written");
        let path = root.0.join("ask-target.txt");
        let response = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":12,"method":"fs/write_text_file","params":{
                        "sessionId":"synthetic-session","path":path.to_str().expect("path"),
                        "content":"must not appear"
                    }
                })),
                &Mapper,
            )
            .expect("refusal");
        assert_eq!(egress_value(&response.egress()[0])["error"]["code"], -32000);
        assert!(response.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Error(crate::acp::intent::NormalizedPayload::Error { .. })
        )));
        assert!(!path.exists());
    }

    #[test]
    fn ask_mode_denial_sends_cancelled_and_leaves_the_target_absent() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-deny-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ask_edit_runtime(&root, "ask-deny");
        let denied = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: scoped_permission_id("ask-deny", 38),
                decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
            })
            .expect("deny");
        assert_eq!(
            egress_value(&denied)["result"]["outcome"]["outcome"],
            "cancelled"
        );
        let token = denied.into_token();
        runtime
            .commit_mandatory_egress(&token)
            .expect("durable decline");
        runtime
            .resolve_egress(token, EgressResolution::Written)
            .expect("decline written");
        assert!(
            !root.0.join("ask-target.txt").exists(),
            "denied synthetic tool must not create the file"
        );
    }

    #[test]
    fn ask_mode_sends_bash_to_the_user_permission_card() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-bash-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let permission = workspace_permission_case(
            &root,
            "ask-bash",
            "execute",
            None,
            crate::settings_store::SessionAccessMode::Ask,
        );
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    fn reject_only_edit_permission(
        mode: crate::settings_store::SessionAccessMode,
        label: &str,
    ) -> Result<crate::acp::bindings::adapter::AdapterBatch, AdapterRuntimeError> {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-reject-only-{label}-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 41, label, OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "edit").expect("prompt"),
                client_message_id: RequiredText::new("client", label).expect("client"),
                access_mode: mode,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"reject-only-tool",
                    "title":"write","kind":"edit","status":"pending","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("tool call");
        let path = root.0.join("new-file.txt");
        let path = path.to_str().expect("path");
        runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","id":0,"method":"session/request_permission","params":{
                "sessionId":"synthetic-session",
                "toolCall":{"toolCallId":"reject-only-tool","title":path,"kind":"edit",
                    "status":"pending","locations":[{"path":path}],"rawInput":{"filepath":path}},
                "options":[{"optionId":"reject","kind":"reject_once","name":"Reject"}]
            }
        })), &Mapper)
    }

    #[test]
    fn read_only_cancels_reject_only_permission_requests() {
        let permission = reject_only_edit_permission(
            crate::settings_store::SessionAccessMode::ReadOnly,
            "reject-only-readonly",
        )
        .expect("read-only must cancel");
        assert_eq!(permission.egress().len(), 1);
        assert_eq!(
            egress_value(&permission.egress()[0])["result"]["outcome"]["outcome"],
            "cancelled"
        );
    }

    #[test]
    fn ask_mode_shows_a_card_for_reject_only_permission_requests() {
        let permission = reject_only_edit_permission(
            crate::settings_store::SessionAccessMode::Ask,
            "reject-only-ask",
        )
        .expect("ask must show a card");
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[test]
    fn workspace_write_shows_a_card_when_auto_allow_is_not_offered() {
        let permission = reject_only_edit_permission(
            crate::settings_store::SessionAccessMode::WorkspaceWrite,
            "reject-only-workspace",
        )
        .expect("workspace write must show a card");
        assert!(permission.egress().is_empty());
        assert!(
            permission
                .events()
                .iter()
                .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
        );
    }

    #[test]
    fn ask_mode_flags_an_edit_that_completed_without_user_approval() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-ask-unsafe-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 39, "ask-unsafe", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read only").expect("prompt"),
                client_message_id: RequiredText::new("client", "ask-unsafe").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::Ask,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        let violated = runtime.ingest(&frame(serde_json::json!({
            "jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"synthetic-session","update":{
                    "sessionUpdate":"tool_call","toolCallId":"unapproved-edit",
                    "title":"write","kind":"edit","status":"completed","locations":[],"rawInput":{}
                }
            }
        })), &Mapper).expect("safety alert");
        assert!(violated.events().iter().any(|event| matches!(event,
            AdapterEvent::Error(NormalizedPayload::Error { code, .. }) if code.as_str() == "askSafetyViolation"
        )));
    }

    #[test]
    fn read_only_unpermitted_edit_completion_raises_a_visible_safety_alert() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-unsafe-completion-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 35, "opencode-unsafe", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read only").expect("prompt"),
                client_message_id: RequiredText::new("client", "unsafe-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("prompt");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        let violated = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","method":"session/update","params":{
                        "sessionId":"synthetic-session","update":{
                            "sessionUpdate":"tool_call","toolCallId":"unexpected-edit",
                            "title":"write","kind":"edit","status":"completed","locations":[],"rawInput":{}
                        }
                    }
                })),
                &Mapper,
            )
            .expect("safety alert batch");
        assert!(violated.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Error(NormalizedPayload::Error { code, .. })
                if code.as_str() == "readOnlySafetyViolation"
        )));
        assert!(violated.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Failed,
                ..
            }
        )));
        assert!(
            violated
                .egress()
                .iter()
                .any(|egress| egress_value(egress)["method"] == "session/cancel")
        );
    }

    #[test]
    fn rejected_read_only_mode_never_emits_an_opencode_prompt() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-mode-reject-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime =
            switching_runtime(&root, 14, "opencode-mode-reject", OPENCODE_OBSERVED_VERSION);
        let select = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "create x.txt").expect("prompt"),
                client_message_id: RequiredText::new("client", "rejected-mode-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("mode request");
        runtime
            .resolve_egress(select.into_token(), EgressResolution::Written)
            .expect("mode written");
        assert!(matches!(
            runtime.ingest(
                &frame(serde_json::json!({
                    "jsonrpc":"2.0","id":3,
                    "error":{"code":-32602,"message":"mode unavailable"}
                })),
                &Mapper,
            ),
            Err(AdapterRuntimeError::ConfigMutationRejected)
        ));
    }

    #[test]
    fn stop_before_mode_ack_cancels_the_deferred_turn_without_prompting() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-stop-mode-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime =
            switching_runtime(&root, 16, "opencode-stop-mode", OPENCODE_OBSERVED_VERSION);
        let select = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "create x.txt").expect("prompt"),
                client_message_id: RequiredText::new("client", "stop-mode-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("mode selection");
        runtime
            .resolve_egress(select.into_token(), EgressResolution::Written)
            .expect("mode written");
        let cancel = runtime
            .command(AdapterCommand::Cancel)
            .expect("stop accepted");
        assert_eq!(egress_value(&cancel)["method"], "session/cancel");
        runtime
            .resolve_egress(cancel.into_token(), EgressResolution::Written)
            .expect("cancel written");
        let ack = runtime
            .ingest(
                &frame(serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}})),
                &Mapper,
            )
            .expect("mode acknowledged after stop");
        assert!(
            ack.egress().is_empty(),
            "stopped mode selection must not release a prompt"
        );
        assert!(ack.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn stop_after_mode_ack_cancels_the_released_prompt() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-stop-after-mode-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = switching_runtime(
            &root,
            18,
            "opencode-stop-after-mode",
            OPENCODE_OBSERVED_VERSION,
        );
        let select = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read the project").expect("prompt"),
                client_message_id: RequiredText::new("client", "stop-after-mode-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("mode selection");
        runtime
            .resolve_egress(select.into_token(), EgressResolution::Written)
            .expect("mode written");
        let selected = runtime
            .ingest(
                &frame(serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}})),
                &Mapper,
            )
            .expect("mode acknowledged");
        assert_eq!(
            egress_value(&selected.egress()[0])["method"],
            "session/prompt"
        );
        runtime
            .resolve_egress(
                selected.egress()[0].token().clone(),
                EgressResolution::Written,
            )
            .expect("prompt written");
        let cancel = runtime
            .command(AdapterCommand::Cancel)
            .expect("stop accepted");
        runtime
            .resolve_egress(cancel.into_token(), EgressResolution::Written)
            .expect("cancel written");
        let settled = runtime
            .ingest(
                &frame(
                    serde_json::json!({"jsonrpc":"2.0","id":4,"result":{"stopReason":"cancelled"}}),
                ),
                &Mapper,
            )
            .expect("prompt cancelled");
        assert!(settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn close_before_mode_ack_never_releases_a_prompt_in_either_response_order() {
        for close_first in [false, true] {
            let root = TempRoot(
                std::env::temp_dir()
                    .join(format!("sage-opencode-close-mode-{}", uuid::Uuid::new_v4())),
            );
            std::fs::create_dir_all(&root.0).expect("create root");
            let mut runtime =
                switching_runtime(&root, 17, "opencode-close-mode", OPENCODE_OBSERVED_VERSION);
            let select = runtime
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "create x.txt").expect("prompt"),
                    client_message_id: RequiredText::new("client", "close-mode-client")
                        .expect("client"),
                    access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                })
                .expect("mode selection");
            runtime
                .resolve_egress(select.into_token(), EgressResolution::Written)
                .expect("mode written");
            let close = runtime
                .command(AdapterCommand::CloseSession)
                .expect("close accepted");
            runtime
                .resolve_egress(close.into_token(), EgressResolution::Written)
                .expect("close written");
            let mode_ack = serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}});
            let close_ack = serde_json::json!({"jsonrpc":"2.0","id":4,"result":{}});
            let first = if close_first {
                close_ack.clone()
            } else {
                mode_ack.clone()
            };
            let second = if close_first { mode_ack } else { close_ack };
            let first_batch = runtime.ingest(&frame(first), &Mapper).expect("first ack");
            let second_batch = runtime.ingest(&frame(second), &Mapper).expect("second ack");
            assert!(first_batch.egress().is_empty() && second_batch.egress().is_empty());
            assert!(
                first_batch
                    .events()
                    .iter()
                    .chain(second_batch.events())
                    .any(|event| matches!(
                        event,
                        AdapterEvent::Turn {
                            state: TurnState::Cancelled,
                            ..
                        }
                    ))
            );
        }
    }

    #[test]
    fn rejected_close_egress_does_not_release_a_deferred_prompt() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-close-rejected-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = switching_runtime(
            &root,
            19,
            "opencode-close-rejected",
            OPENCODE_OBSERVED_VERSION,
        );
        let select = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "create x.txt").expect("prompt"),
                client_message_id: RequiredText::new("client", "close-rejected-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("mode selection");
        runtime
            .resolve_egress(select.into_token(), EgressResolution::Written)
            .expect("mode written");
        let close = runtime
            .command(AdapterCommand::CloseSession)
            .expect("close request");
        runtime
            .resolve_egress(close.into_token(), EgressResolution::QueueRejected)
            .expect("close rejected");
        let ack = runtime
            .ingest(
                &frame(serde_json::json!({"jsonrpc":"2.0","id":3,"result":{}})),
                &Mapper,
            )
            .expect("mode acknowledged");
        assert!(ack.egress().is_empty());
        assert!(ack.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Cancelled,
                ..
            }
        )));
    }

    #[test]
    fn missing_read_only_mode_cannot_send_an_opencode_prompt() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-mode-missing-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime_with_mode(
            &root,
            15,
            "opencode-mode-missing",
            OPENCODE_OBSERVED_VERSION,
            false,
            "plan",
        );
        assert!(matches!(
            runtime.command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "create x.txt").expect("prompt"),
                client_message_id: RequiredText::new("client", "missing-mode-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            }),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
        assert!(matches!(
            runtime.command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "read this repo").expect("prompt"),
                client_message_id: RequiredText::new("client", "build-client").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            }),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
    }

    #[test]
    fn an_admitted_opencode_session_closes_through_the_native_session_close_request() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-close-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 6, "opencode-close", OPENCODE_OBSERVED_VERSION);

        let close = runtime
            .command(AdapterCommand::CloseSession)
            .expect("close command");
        let value = egress_value(&close);
        assert_eq!(value["method"], "session/close");
        assert_eq!(value["params"]["sessionId"], "synthetic-session");
        assert_eq!(value["id"], 3);
        runtime
            .resolve_egress(close.into_token(), EgressResolution::Written)
            .expect("close written");
        let acknowledged = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "result": {}
                })),
                &Mapper,
            )
            .expect("close response");

        assert!(matches!(
            acknowledged.events(),
            [AdapterEvent::Acknowledged]
        ));
    }

    #[test]
    fn the_negotiated_opencode_surface_matches_the_curated_registry_snapshot() {
        let characterised = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-agree-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&characterised.0).expect("create root");
        let observed = ready_runtime(
            &characterised,
            7,
            "opencode-agree",
            OPENCODE_OBSERVED_VERSION,
        );

        assert_eq!(
            observed.capabilities().expect("negotiated snapshot"),
            curated_registry_snapshot(&characterised, OPENCODE_OBSERVED_VERSION)
        );

        let uncharacterised = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-agree-next-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&uncharacterised.0).expect("create root");
        let next = ready_runtime(&uncharacterised, 8, "opencode-agree-next", "1.18.22");

        assert_eq!(
            next.capabilities().expect("negotiated snapshot"),
            curated_registry_snapshot(&uncharacterised, "1.18.22")
        );
    }

    #[test]
    fn an_opencode_close_acknowledged_before_the_prompt_response_still_settles_the_turn() {
        let root = TempRoot(
            std::env::temp_dir().join(format!("sage-opencode-close-race-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(&root, 9, "opencode-close-race", OPENCODE_OBSERVED_VERSION);
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "synthetic prompt").expect("prompt"),
                client_message_id: RequiredText::new("client", "close-race-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        let close = runtime
            .command(AdapterCommand::CloseSession)
            .expect("close command");
        assert_eq!(egress_value(&close)["id"], 4);
        runtime
            .resolve_egress(close.into_token(), EgressResolution::Written)
            .expect("close written");

        let acknowledged = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "result": {}
                })),
                &Mapper,
            )
            .expect("close response");
        assert!(matches!(
            acknowledged.events(),
            [AdapterEvent::Acknowledged]
        ));
        let settled = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "result": {"stopReason": "end_turn"}
                })),
                &Mapper,
            )
            .expect("prompt response after close acknowledgement");

        assert!(settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        )));
        assert!(matches!(
            runtime.command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "after close").expect("prompt"),
                client_message_id: RequiredText::new("client", "after-close-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            }),
            Err(AdapterRuntimeError::Operation)
        ));
    }

    #[test]
    fn an_opencode_prompt_response_before_the_close_acknowledgement_settles_both() {
        let root = TempRoot(std::env::temp_dir().join(format!(
            "sage-opencode-close-settled-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&root.0).expect("create root");
        let mut runtime = ready_runtime(
            &root,
            10,
            "opencode-close-settled",
            OPENCODE_OBSERVED_VERSION,
        );
        let prompt = runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "synthetic prompt").expect("prompt"),
                client_message_id: RequiredText::new("client", "close-settled-client")
                    .expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::WorkspaceWrite,
            })
            .expect("prompt command");
        runtime
            .resolve_egress(prompt.into_token(), EgressResolution::Written)
            .expect("prompt written");
        let close = runtime
            .command(AdapterCommand::CloseSession)
            .expect("close command");
        runtime
            .resolve_egress(close.into_token(), EgressResolution::Written)
            .expect("close written");

        let settled = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "result": {"stopReason": "end_turn"}
                })),
                &Mapper,
            )
            .expect("prompt response");
        let acknowledged = runtime
            .ingest(
                &frame(serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 4,
                    "result": {}
                })),
                &Mapper,
            )
            .expect("close response");

        assert!(settled.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        )));
        assert!(matches!(
            acknowledged.events(),
            [AdapterEvent::Acknowledged]
        ));
    }

    #[path = "created_file_tests.rs"]
    mod created_file_tests;
}
