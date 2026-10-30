#[cfg(test)]
use crate::harness_discovery::{KnownHarnessSignIn, known_harness_spec};

use super::surface::UnmappedUpdate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StandardAcpLaunch {
    NodeEntrypoint,
    DirectExecutable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StandardAcpVersionShape {
    DottedNumeric,
    DateBuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StandardAcpClientCapabilities {
    pub(crate) read_text_file: bool,
    pub(crate) write_text_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NpmPackageManifestRequirement {
    pub(crate) package_dir: &'static str,
    pub(crate) name: &'static str,
    pub(crate) bin_name: &'static str,
    pub(crate) bin_path: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StandardAcpProfile {
    pub(crate) harness_id: &'static str,
    pub(crate) binding_id: &'static str,
    pub(crate) launch: StandardAcpLaunch,
    pub(crate) fixed_args: &'static [&'static str],
    pub(crate) version_probe_args: &'static [&'static str],
    pub(crate) version_shape: StandardAcpVersionShape,
    pub(crate) client_capabilities: StandardAcpClientCapabilities,
    pub(crate) package_manifest: Option<NpmPackageManifestRequirement>,
    pub(crate) other_tool_label: &'static str,
    pub(crate) command_input_field: Option<&'static str>,
    pub(crate) task_tool_lifecycle: bool,
    pub(crate) vendor_updates: &'static [(&'static str, UnmappedUpdate)],
    pub(crate) passthrough_env: &'static [&'static str],
    pub(crate) remediation: &'static str,
}

const GEMINI_PASSTHROUGH_ENV: &[&str] = &[
    "GOOGLE_CLOUD_PROJECT",
    "GOOGLE_CLOUD_LOCATION",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
];

const CLAUDE_VENDOR_UPDATES: &[(&str, UnmappedUpdate)] = &[
    (
        "config_option_update",
        UnmappedUpdate::Surfaced(
            "Claude reported a configuration option change; Sage reads session configuration from its own negotiated state and grants the harness no authority over it",
        ),
    ),
    (
        "claude_nested_transcript",
        UnmappedUpdate::Surfaced(
            "Nested Claude transcript metadata is display-only and is not admitted as transcript authority",
        ),
    ),
    (
        "async_task_spawned",
        UnmappedUpdate::Surfaced(
            "Claude spawned a background task; Sage models collaboration through activities and does not adopt an unproven task identity",
        ),
    ),
    (
        "async_task_progress",
        UnmappedUpdate::Surfaced(
            "Claude reported progress for a background task Sage did not adopt",
        ),
    ),
    (
        "async_task_state_update",
        UnmappedUpdate::Surfaced(
            "Claude reported a state change for a background task Sage did not adopt",
        ),
    ),
    (
        "subagent_spawned",
        UnmappedUpdate::Surfaced(
            "Claude spawned a subagent; Sage models collaboration through activities and does not adopt an unproven subagent identity",
        ),
    ),
    (
        "subagent_state_update",
        UnmappedUpdate::Surfaced(
            "Claude reported a state change for a subagent Sage did not adopt",
        ),
    ),
];

const GEMINI_VENDOR_UPDATES: &[(&str, UnmappedUpdate)] = &[];

const CURSOR_VENDOR_UPDATES: &[(&str, UnmappedUpdate)] = &[];

const OPENCODE_VENDOR_UPDATES: &[(&str, UnmappedUpdate)] = &[];

pub(crate) const PROFILES: &[StandardAcpProfile] = &[
    StandardAcpProfile {
        harness_id: "claude-code",
        binding_id: "claude-agent-acp",
        launch: StandardAcpLaunch::NodeEntrypoint,
        fixed_args: &[],
        version_probe_args: &["--version"],
        version_shape: StandardAcpVersionShape::DottedNumeric,
        client_capabilities: StandardAcpClientCapabilities {
            read_text_file: true,
            write_text_file: false,
        },
        package_manifest: None,
        other_tool_label: "Claude tool",
        command_input_field: Some("command"),
        task_tool_lifecycle: true,
        vendor_updates: CLAUDE_VENDOR_UPDATES,
        passthrough_env: &[],
        remediation: "Sign in by running claude auth login. Sage reuses that login from your Claude configuration directory.",
    },
    StandardAcpProfile {
        harness_id: "gemini-cli",
        binding_id: "gemini-cli-acp",
        launch: StandardAcpLaunch::NodeEntrypoint,
        fixed_args: &["--experimental-acp"],
        version_probe_args: &["--version"],
        version_shape: StandardAcpVersionShape::DottedNumeric,
        client_capabilities: StandardAcpClientCapabilities {
            read_text_file: true,
            write_text_file: false,
        },
        package_manifest: None,
        other_tool_label: "Gemini tool",
        command_input_field: None,
        task_tool_lifecycle: false,
        vendor_updates: GEMINI_VENDOR_UPDATES,
        passthrough_env: GEMINI_PASSTHROUGH_ENV,
        remediation: "Sign in by running gemini, or set GOOGLE_CLOUD_PROJECT before starting Sage if the account is a Google Workspace account.",
    },
    StandardAcpProfile {
        harness_id: "cursor-agent",
        binding_id: "cursor-agent-acp",
        launch: StandardAcpLaunch::NodeEntrypoint,
        fixed_args: &["acp"],
        version_probe_args: &["--version"],
        version_shape: StandardAcpVersionShape::DateBuild,
        client_capabilities: StandardAcpClientCapabilities {
            read_text_file: true,
            write_text_file: false,
        },
        package_manifest: None,
        other_tool_label: "Cursor tool",
        command_input_field: None,
        task_tool_lifecycle: false,
        vendor_updates: CURSOR_VENDOR_UPDATES,
        passthrough_env: &[],
        remediation: "Sign in by running cursor-agent login.",
    },
    StandardAcpProfile {
        harness_id: "opencode",
        binding_id: "opencode-acp",
        launch: StandardAcpLaunch::DirectExecutable,
        fixed_args: &["acp"],
        version_probe_args: &["--version"],
        version_shape: StandardAcpVersionShape::DottedNumeric,
        client_capabilities: StandardAcpClientCapabilities {
            read_text_file: false,
            write_text_file: true,
        },
        package_manifest: Some(NpmPackageManifestRequirement {
            package_dir: "opencode-ai",
            name: "opencode-ai",
            bin_name: "opencode",
            bin_path: "./bin/opencode.exe",
        }),
        other_tool_label: "OpenCode tool",
        command_input_field: None,
        task_tool_lifecycle: false,
        vendor_updates: OPENCODE_VENDOR_UPDATES,
        passthrough_env: &[],
        remediation: "OpenCode ACP is unavailable in Sage until its live characterization is complete.",
    },
];

pub(crate) fn profile_for(harness_id: &str) -> Option<&'static StandardAcpProfile> {
    PROFILES
        .iter()
        .find(|profile| profile.harness_id == harness_id)
}

impl StandardAcpVersionShape {
    pub(crate) fn accepts(self, value: &str) -> bool {
        match self {
            Self::DottedNumeric => dotted_numeric(value),
            Self::DateBuild => date_build(value),
        }
    }
}

impl StandardAcpProfile {
    #[cfg(test)]
    pub(crate) fn auth_hint(self) -> Option<KnownHarnessSignIn> {
        known_harness_spec(self.harness_id).and_then(|spec| spec.sign_in)
    }
}

fn dotted_numeric(value: &str) -> bool {
    let mut parts = value.split('.');
    (0..3).all(|_| parts.next().is_some_and(numeric_component)) && parts.next().is_none()
}

fn date_build(value: &str) -> bool {
    let Some((date, build)) = value.split_once('-') else {
        return false;
    };
    let mut parts = date.split('.');
    let valid_date =
        (0..3).all(|_| parts.next().is_some_and(numeric_component)) && parts.next().is_none();
    valid_date
        && !build.is_empty()
        && build.len() <= 64
        && build.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn numeric_component(value: &str) -> bool {
    !value.is_empty() && value.len() <= 10 && value.bytes().all(|byte| byte.is_ascii_digit())
}
