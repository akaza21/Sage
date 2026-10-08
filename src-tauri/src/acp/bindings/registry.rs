//! Curated native binding metadata and admission authority, not runtime ownership.

use std::sync::Arc;

use crate::acp::bindings::{
    claude::{CLAUDE_BINDING_ID, ClaudeAdmissionUnavailable, ClaudeBindingRegistration},
    codex::{
        CODEX_BINDING_ID, CODEX_PROTOCOL_IDENTITY, CODEX_SCHEMA_SHA256, CodexAdmissionUnavailable,
        CodexBindingRegistration, startup_codex_binding_registration,
    },
    standard_acp::{
        OPENCODE_BINDING_ID, OpenCodeAdmissionUnavailable, OpenCodeBindingRegistration,
    },
};

mod claude;
mod codex;
mod opencode;
mod record;
use record::{Admission, AdmittedBinding, RootPolicy};
pub(crate) use record::{AdmittedBindingRecord, BindingProjectRoot, BindingRecord};
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::{
    TEST_BINDING_ID, take_test_cancel_dispatch_for_test, take_test_close_dispatch_for_test,
    take_test_factory_construction_for_test, take_test_turn_dispatch_for_test,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ModelProvider {
    CodexAccount,
    ClaudeAdapter,
    NoAccountModels,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum AuthProvider {
    Codex,
    Claude,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WireIdentity {
    pub(crate) protocol: &'static str,
    pub(crate) schema_sha256: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryAdmissionUnavailable {
    #[error(transparent)]
    Codex(CodexAdmissionUnavailable),
    #[error(transparent)]
    Claude(ClaudeAdmissionUnavailable),
    #[error(transparent)]
    OpenCode(OpenCodeAdmissionUnavailable),
    #[error("binding wire identity drifted after admission")]
    Drift,
}

impl From<RegistryAdmissionUnavailable> for CodexAdmissionUnavailable {
    fn from(value: RegistryAdmissionUnavailable) -> Self {
        match value {
            RegistryAdmissionUnavailable::Codex(reason) => reason,
            RegistryAdmissionUnavailable::Claude(_)
            | RegistryAdmissionUnavailable::OpenCode(_)
            | RegistryAdmissionUnavailable::Drift => CodexAdmissionUnavailable::Drift,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryUnavailable {
    UnknownBinding,
    Admission(CodexAdmissionUnavailable),
}

pub(crate) struct BindingRegistry {
    codex: BindingRecord,
    claude: BindingRecord,
    opencode: BindingRecord,
    #[cfg(test)]
    test_binding: Option<BindingRecord>,
}

impl Default for BindingRegistry {
    fn default() -> Self {
        Self::new(
            startup_codex_binding_registration(),
            ClaudeBindingRegistration::startup(),
        )
    }
}

impl BindingRegistry {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn desktop_codex_fixture(
        selection: crate::launch_profile::CodexFixtureSelection,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        let codex = CodexBindingRegistration::desktop_fixture(selection)?;
        let mut registry = Self::new(codex, ClaudeBindingRegistration::startup());
        registry.codex.model_provider = ModelProvider::NoAccountModels;
        registry.codex.auth_provider = AuthProvider::None;
        Ok(registry)
    }

    pub(crate) fn new(codex: CodexBindingRegistration, claude: ClaudeBindingRegistration) -> Self {
        Self::new_with_opencode(codex, claude, OpenCodeBindingRegistration::startup())
    }

    fn new_with_opencode(
        codex: CodexBindingRegistration,
        claude: ClaudeBindingRegistration,
        opencode: OpenCodeBindingRegistration,
    ) -> Self {
        Self {
            codex: BindingRecord {
                harness_id: "codex",
                binding_id: CODEX_BINDING_ID,
                wire: Some(WireIdentity {
                    protocol: CODEX_PROTOCOL_IDENTITY,
                    schema_sha256: CODEX_SCHEMA_SHA256,
                }),
                root_policy: RootPolicy::Codex,
                session_operations_enabled: true,
                admission: Admission::Codex(codex),
                launch_factory: codex::launch_factory,
                supported_capabilities: codex::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::CodexAccount,
                auth_provider: AuthProvider::Codex,
            },
            claude: BindingRecord {
                harness_id: "claude-code",
                binding_id: CLAUDE_BINDING_ID,
                wire: None,
                root_policy: RootPolicy::Claude,
                session_operations_enabled: true,
                admission: Admission::Claude(Arc::new(claude)),
                launch_factory: claude::launch_factory,
                supported_capabilities: claude::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::ClaudeAdapter,
                auth_provider: AuthProvider::Claude,
            },
            opencode: BindingRecord {
                harness_id: "opencode",
                binding_id: OPENCODE_BINDING_ID,
                wire: None,
                root_policy: RootPolicy::OpenCode,
                session_operations_enabled: true,
                admission: Admission::OpenCode(Arc::new(opencode)),
                launch_factory: opencode::launch_factory,
                supported_capabilities: opencode::capabilities,
                turn_dispatch: crate::acp::service::AcpService::send_session_turn,
                cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
                close_dispatch: crate::acp::service::AcpService::close_session,
                model_provider: ModelProvider::NoAccountModels,
                auth_provider: AuthProvider::None,
            },
            #[cfg(test)]
            test_binding: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_codex(codex: CodexBindingRegistration) -> Self {
        Self::new(codex, ClaudeBindingRegistration::absent_for_test())
    }

    pub(crate) fn codex(&self) -> &BindingRecord {
        &self.codex
    }

    pub(crate) fn codex_admission(&self) -> &CodexBindingRegistration {
        let Admission::Codex(admission) = &self.codex.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn claude_admission(&self) -> &Arc<ClaudeBindingRegistration> {
        let Admission::Claude(admission) = &self.claude.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn claude(&self) -> &BindingRecord {
        &self.claude
    }

    pub(crate) fn opencode(&self) -> &BindingRecord {
        &self.opencode
    }

    pub(crate) fn opencode_admission(&self) -> &Arc<OpenCodeBindingRegistration> {
        let Admission::OpenCode(admission) = &self.opencode.admission else {
            unreachable!()
        };
        admission
    }

    pub(crate) fn for_harness(&self, id: &str) -> Option<&BindingRecord> {
        [self.codex(), self.claude(), self.opencode()]
            .into_iter()
            .find(|record| record.harness_id() == id)
            .or({
                #[cfg(test)]
                {
                    self.test_binding
                        .as_ref()
                        .filter(|record| record.harness_id() == id)
                }
                #[cfg(not(test))]
                {
                    None
                }
            })
    }

    pub(crate) fn resolve(
        &self,
        binding_id: &str,
    ) -> Result<AdmittedBindingRecord<'_>, RegistryUnavailable> {
        self.route(binding_id)?
            .admit()
            .map_err(|reason| RegistryUnavailable::Admission(reason.into()))
    }

    pub(crate) fn route(&self, binding_id: &str) -> Result<&BindingRecord, RegistryUnavailable> {
        [&self.codex, &self.claude, &self.opencode]
            .into_iter()
            .find(|record| record.binding_id() == binding_id)
            .or({
                #[cfg(test)]
                {
                    self.test_binding
                        .as_ref()
                        .filter(|record| record.binding_id() == binding_id)
                }
                #[cfg(not(test))]
                {
                    None
                }
            })
            .ok_or(RegistryUnavailable::UnknownBinding)
    }
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BindingRegistry")
            .field("codex_available", &self.codex.admission_available())
            .field("claude_available", &self.claude.admission_available())
            .field("opencode_available", &self.opencode.admission_available())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
impl BindingRegistry {
    pub(crate) fn new_with_opencode_for_test(
        codex: CodexBindingRegistration,
        claude: ClaudeBindingRegistration,
        opencode: OpenCodeBindingRegistration,
    ) -> Self {
        Self::new_with_opencode(codex, claude, opencode)
    }

    #[cfg(windows)]
    pub(crate) fn with_test_binding(
        codex: CodexBindingRegistration,
        test_binding: CodexBindingRegistration,
    ) -> Self {
        let mut registry = Self::with_codex(codex);
        registry.test_binding = Some(BindingRecord {
            harness_id: "registry-test",
            binding_id: test_support::TEST_BINDING_ID,
            wire: Some(WireIdentity {
                protocol: CODEX_PROTOCOL_IDENTITY,
                schema_sha256: CODEX_SCHEMA_SHA256,
            }),
            root_policy: RootPolicy::Codex,
            session_operations_enabled: true,
            admission: Admission::Codex(test_binding),
            launch_factory: test_support::launch_factory,
            supported_capabilities: codex::capabilities,
            turn_dispatch: test_support::turn_dispatch,
            cancel_dispatch: test_support::cancel_dispatch,
            close_dispatch: test_support::close_dispatch,
            model_provider: ModelProvider::CodexAccount,
            auth_provider: AuthProvider::Codex,
        });
        registry
    }

    #[cfg(windows)]
    pub(crate) fn test_registration(&self) -> Option<CodexBindingRegistration> {
        match self.test_binding.as_ref()?.admission {
            Admission::Codex(ref admission) => Some(admission.clone()),
            Admission::Claude(_) | Admission::OpenCode(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, fs};

    use crate::{
        acp::{
            bindings::standard_acp::OpenCodeBindingRegistration,
            contract::{
                ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
            },
            process::RequiredContainment,
        },
        project_registry::ProjectRootAuthority,
    };

    #[test]
    fn registry_resolves_claude_record_only_when_injectable_admission_is_present() {
        // This discriminates because the registry must route the Claude binding
        // and invoke its real registration, rather than merely cataloguing an ID.
        let registry = BindingRegistry::default();
        assert!(
            matches!(registry.route(CLAUDE_BINDING_ID), Ok(record) if record.harness_id() == "claude-code")
        );
    }

    #[cfg(windows)]
    #[test]
    fn opencode_registry_resolution_constructs_an_exact_direct_native_launch() {
        let root =
            std::env::temp_dir().join(format!("sage-opencode-registry-{}", uuid::Uuid::new_v4()));
        let npm = root.join("npm");
        let shim = npm.join("opencode.cmd");
        let executable = npm
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
        fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
        fs::write(&executable, b"native-opencode").expect("write runtime");
        fs::write(
            npm.join("node_modules")
                .join("opencode-ai")
                .join("package.json"),
            r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
        )
        .expect("write manifest");
        let registration = OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21");
        let registry = BindingRegistry::new_with_opencode_for_test(
            CodexBindingRegistration::unavailable_for_test(),
            ClaudeBindingRegistration::absent_for_test(),
            registration,
        );
        let shim_marker = root.join("shim-ran");
        fs::write(
            &shim,
            format!(
                "@echo off\r\necho executed>\"{}\"\r\necho 1.18.21\r\n",
                shim_marker.display()
            ),
        )
        .expect("replace shim after admission");
        let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
        let (expected_binding_digest, expected_artifact_digest) = {
            let identity = admitted.identity();
            (identity.binding_digest, identity.artifact_digest.to_owned())
        };
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-registry").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-registry").expect("root"),
            &root,
        )
        .expect("project root");
        let mut runtime = admitted
            .into_factory(
                BindingProjectRoot::OpenCode(project_root),
                crate::acp::mcp::McpAdmission::default(),
            )
            .construct(
                RuntimeEpoch::new(17).expect("epoch"),
                SageAgentSessionId::parse("session-opencode-registry").expect("session"),
            )
            .expect("construct runtime");
        let spec = runtime.launch_spec().expect("launch spec");

        assert_eq!(spec.binding_id.as_bytes(), &expected_binding_digest[..16]);
        assert_eq!(spec.artifact.canonical_absolute_path, executable);
        assert_eq!(
            spec.artifact
                .sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            expected_artifact_digest
        );
        assert_eq!(spec.fixed_args.as_slice(), [OsStr::new("acp")]);
        let mut expected_environment: Vec<_> = [
            "APPDATA",
            "ComSpec",
            "LOCALAPPDATA",
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "WINDIR",
        ]
        .into_iter()
        .filter_map(|key| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(|value| (std::ffi::OsString::from(key), value))
        })
        .collect();
        let (policy_key, policy_content) = spec
            .environment
            .as_slice()
            .last()
            .expect("OpenCode policy environment");
        assert_eq!(policy_key, "OPENCODE_CONFIG_CONTENT");
        let policy_text = policy_content.to_str().expect("UTF-8 OpenCode policy");
        let policy: serde_json::Value = serde_json::from_str(policy_text).expect("policy JSON");
        let expected_policy = serde_json::json!({"permission":{
            "*":"ask", "bash":"ask", "edit":"ask", "read":"allow",
            "task":"ask", "webfetch":"ask", "write":"ask"
        }});
        assert_eq!(policy, expected_policy);
        assert_eq!(
            policy_text,
            serde_json::to_string(&expected_policy).unwrap()
        );
        expected_environment.push((
            std::ffi::OsString::from("OPENCODE_CONFIG_CONTENT"),
            std::ffi::OsString::from(policy_text),
        ));
        assert_eq!(spec.environment.as_slice(), expected_environment);
        assert_eq!(spec.required_guarantee, RequiredContainment::EnforcedTree);
        assert!(!shim_marker.exists());
        drop(spec);
        fs::remove_dir_all(root).expect("remove root");
    }

    #[cfg(windows)]
    #[test]
    fn opencode_factory_refuses_runtime_drift_immediately_before_launch() {
        let root = std::env::temp_dir().join(format!(
            "sage-opencode-registry-drift-{}",
            uuid::Uuid::new_v4()
        ));
        let npm = root.join("npm");
        let shim = npm.join("opencode.cmd");
        let executable = npm
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
        fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
        fs::write(&executable, b"native-opencode").expect("write runtime");
        fs::write(
            npm.join("node_modules")
                .join("opencode-ai")
                .join("package.json"),
            r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
        )
        .expect("write manifest");
        let registry = BindingRegistry::new_with_opencode_for_test(
            CodexBindingRegistration::unavailable_for_test(),
            ClaudeBindingRegistration::absent_for_test(),
            OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21"),
        );
        let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
        let project_root = ProjectRootAuthority::for_test(
            SageProjectId::parse("project-opencode-registry-drift").expect("project"),
            ProjectGeneration::ZERO,
            SageRootId::parse("root-opencode-registry-drift").expect("root"),
            &root,
        )
        .expect("project root");
        let mut runtime = admitted
            .into_factory(
                BindingProjectRoot::OpenCode(project_root),
                crate::acp::mcp::McpAdmission::default(),
            )
            .construct(
                RuntimeEpoch::new(18).expect("epoch"),
                SageAgentSessionId::parse("session-opencode-registry-drift").expect("session"),
            )
            .expect("construct runtime");
        fs::write(&executable, b"changed-opencode").expect("change runtime");

        assert!(matches!(
            runtime.launch_spec(),
            Err(crate::acp::bindings::adapter::AdapterRuntimeError::LaunchUnavailable)
        ));
        fs::remove_dir_all(root).expect("remove root");
    }

    #[cfg(windows)]
    #[test]
    fn admitted_opencode_offers_the_observed_mandatory_session_core() {
        let root = std::env::temp_dir().join(format!(
            "sage-opencode-mandatory-core-{}",
            uuid::Uuid::new_v4()
        ));
        let npm = root.join("npm");
        let shim = npm.join("opencode.cmd");
        let executable = npm
            .join("node_modules")
            .join("opencode-ai")
            .join("bin")
            .join("opencode.exe");
        fs::create_dir_all(executable.parent().expect("runtime parent")).expect("create layout");
        fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").expect("write shim");
        fs::write(&executable, b"native-opencode").expect("write runtime");
        fs::write(
            npm.join("node_modules")
                .join("opencode-ai")
                .join("package.json"),
            r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
        )
        .expect("write manifest");
        let registry = BindingRegistry::new_with_opencode_for_test(
            CodexBindingRegistration::unavailable_for_test(),
            ClaudeBindingRegistration::absent_for_test(),
            OpenCodeBindingRegistration::admitted_for_test(&shim, "1.18.21"),
        );
        let admitted = registry.resolve("opencode-acp").expect("resolve OpenCode");
        let snapshot = admitted
            .supported_capabilities()
            .expect("an admitted OpenCode build projects the mandatory session core");

        assert!(snapshot.new_session);
        assert!(snapshot.prompt);
        assert!(snapshot.cancel);
        assert!(snapshot.close_session);
        assert!(!snapshot.resume_session);
        assert!(!snapshot.load_session);
        assert_eq!(snapshot.model_count, 0);
        assert_eq!(snapshot.config_count, 0);
        assert!(
            registry
                .route("opencode-acp")
                .expect("route OpenCode")
                .session_operations_enabled()
        );
        drop(admitted);
        fs::remove_dir_all(root).expect("remove root");
    }
}
