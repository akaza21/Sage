//! Curated registry record for Claude Code over its Agent SDK protocol.

use std::sync::Arc;

use crate::acp::{
    bindings::{
        claude::sdk::{
            CLAUDE_SDK_BINDING_ID, ClaudeSdkBindingRegistration, ClaudeSdkRuntimeFactory,
        },
        runtime::RuntimeFactory,
    },
    contract::{RequiredText, SchemaVersionV1},
    intent::CapabilitySnapshotV1,
};

use super::{
    Admission, AdmittedBinding, AuthProvider, BindingProjectRoot, BindingRecord, ModelProvider,
    RegistryAdmissionUnavailable, RootPolicy,
};

pub(super) fn record(registration: Arc<ClaudeSdkBindingRegistration>) -> BindingRecord {
    BindingRecord {
        harness_id: "claude-code",
        binding_id: CLAUDE_SDK_BINDING_ID,
        wire: None,
        root_policy: RootPolicy::Claude,
        session_operations_enabled: true,
        admission: Admission::ClaudeSdk(registration),
        launch_factory,
        supported_capabilities: capabilities,
        turn_dispatch: crate::acp::service::AcpService::send_session_turn,
        cancel_dispatch: crate::acp::service::AcpService::cancel_session_turn,
        close_dispatch: crate::acp::service::AcpService::close_session,
        model_provider: ModelProvider::ClaudeAdapter,
        auth_provider: AuthProvider::Claude,
    }
}

/// A desktop fixture record: the replay case stands in for Claude Code and
/// no Claude sign-in is consulted.
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(super) fn fixture_record(
    selection: crate::launch_profile::FixtureSelection,
) -> Result<BindingRecord, crate::acp::bindings::adapter::AdapterRuntimeError> {
    let registration = ClaudeSdkBindingRegistration::desktop_fixture(selection)?;
    let mut record = record(Arc::new(registration));
    record.auth_provider = AuthProvider::None;
    Ok(record)
}

fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
    mcp: crate::acp::mcp::McpAdmission,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::ClaudeSdk(binding), BindingProjectRoot::Claude(root)) = (binding, root)
    else {
        unreachable!("Claude SDK record receives only Claude SDK admission and root authority")
    };
    // Sage's browser tools reach Claude Code over MCP in a later stage.
    drop(mcp);
    Box::new(ClaudeSdkRuntimeFactory { binding, root })
}

/// What the SDK protocol carries today: new and resumed sessions, prompts
/// with attachments, cancel, and subagent transcripts. Closing ends the
/// process; fork arrives with its own stage.
fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::ClaudeSdk(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .revalidate()
        .map_err(|_| RegistryAdmissionUnavailable::Drift)?;
    ceiling()
}

fn ceiling() -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    Ok(CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "1")
            .map_err(|_| RegistryAdmissionUnavailable::Drift)?,
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: false,
        prompt: true,
        cancel: true,
        native_session_fork: true,
        native_session_compact: true,
        native_session_import: false,
        native_review: false,
        active_turn_steer: true,
        child_conversation_projection: false,
        child_transcript_read: true,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: true,
        prompt_images: true,
        config_count: 0,
    })
}

/// Whether a binding without model discovery can start a session with
/// `model`: Claude Code takes a model its build listed before the session.
pub(super) fn accepts_launch_model(binding: &AdmittedBinding, model: &str) -> bool {
    match binding {
        AdmittedBinding::ClaudeSdk(binding) => {
            crate::acp::bindings::claude::sdk::offers_model(binding, model)
        }
        _ => false,
    }
}

/// The config choice a new session of this binding starts with for a
/// thinking level chosen before it existed, when the binding lists that
/// level for `model` (Claude Code's own default when none is named).
pub(super) fn launch_effort_choice(
    binding: &AdmittedBinding,
    model: Option<&str>,
    effort: &str,
) -> Option<crate::acp::session_config::ConfigMutation> {
    use crate::acp::{
        bindings::claude::sdk,
        session_config::{ConfigId, ConfigMutation, ConfigValue},
    };
    let AdmittedBinding::ClaudeSdk(binding) = binding else {
        return None;
    };
    sdk::offers_effort(binding, model.unwrap_or("default"), effort).then_some(())?;
    Some(ConfigMutation::Option {
        config_id: ConfigId::new("configId", sdk::EFFORT_CONFIG_ID).ok()?,
        value: ConfigValue::Select {
            value: ConfigId::new("value", effort).ok()?,
        },
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_sdk_route_admits_the_image_blocks_its_bridge_forwards() {
        assert!(super::ceiling().expect("ceiling").prompt_images);
    }
}
