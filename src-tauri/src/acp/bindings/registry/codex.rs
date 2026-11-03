#[cfg(all(test, windows))]
use std::sync::Arc;

use crate::acp::{
    bindings::{codex::CodexRuntimeFactory, runtime::RuntimeFactory},
    intent::CapabilitySnapshotV1,
};

use super::{AdmittedBinding, BindingProjectRoot, RegistryAdmissionUnavailable};

#[cfg(all(test, windows))]
use crate::acp::bindings::codex::AdmittedCodexBinding;

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Codex(binding), BindingProjectRoot::Codex(root)) = (binding, root) else {
        unreachable!("curated Codex record receives only Codex admission and root authority")
    };
    Box::new(CodexRuntimeFactory::with_mcp(binding, root, mcp))
}

pub(super) fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::Codex(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .capability_ceiling()
        .map_err(RegistryAdmissionUnavailable::Codex)
}

#[cfg(all(test, windows))]
pub(super) fn binding(binding: &AdmittedBinding) -> Option<&Arc<AdmittedCodexBinding>> {
    match binding {
        AdmittedBinding::Codex(binding) => Some(binding),
        AdmittedBinding::Claude(_)
        | AdmittedBinding::OpenCode { .. }
        | AdmittedBinding::PiNative(_) => None,
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        AdmittedBinding::Pi(_) => None,
    }
}

#[cfg(all(test, windows))]
mod browser_mcp_tests {
    use super::*;
    use crate::acp::{
        bindings::codex::{CodexBindingRegistration, CodexProjectRootAuthority},
        contract::{
            ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageProjectId, SageRootId,
        },
        mcp::McpAdmission,
    };
    use crate::project_registry::ProjectRootAuthority;

    #[test]
    fn codex_launch_forwards_browser_token_in_environment() {
        let registration = CodexBindingRegistration::admitted_for_test();
        let binding = registration.refresh().unwrap();
        let project_root = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                SageProjectId::parse("project-codex-browser").unwrap(),
                ProjectGeneration::ZERO,
                SageRootId::parse("root-codex-browser").unwrap(),
                &std::env::current_dir().unwrap(),
            )
            .unwrap(),
        );
        let mcp = McpAdmission::browser_stdio(
            r"C:\Sage\sage.exe".into(),
            [
                ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
                ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
                (
                    "SAGE_BROWSER_MCP_SESSION".into(),
                    "session-codex-browser".into(),
                ),
                ("SAGE_BROWSER_MCP_TOKEN".into(), "secret-token".into()),
            ],
        )
        .unwrap();
        let mut runtime = launch_factory(
            AdmittedBinding::Codex(binding),
            BindingProjectRoot::Codex(project_root),
            mcp,
        )
        .construct(
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("session-codex-browser").unwrap(),
        )
        .unwrap();
        let spec = runtime.launch_spec().unwrap();
        assert!(
            spec.environment.as_slice().iter().any(|(name, value)| {
                name == "SAGE_BROWSER_MCP_TOKEN" && value == "secret-token"
            })
        );
    }
}
