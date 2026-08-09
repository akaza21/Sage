use serde::Deserialize;

use crate::acp::limits::LIMITS;

use super::manifest::{MetadataValidatedPacket, StatePolicy};

pub(super) const SECURITY_POLICY_BYTES: usize = 16 * 1024;
pub(super) const APPROVAL_POLICY_NEVER: &str = "never";
pub(super) const SANDBOX_READ_ONLY: &str = "readOnly";
pub(super) const SANDBOX_NETWORK_ACCESS: bool = false;

const ALLOWED_ENVIRONMENT_KEYS: [&str; 5] = ["CODEX_HOME", "SystemRoot", "TEMP", "TMP", "WINDIR"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CodexSecurityPolicy {
    schema_version: u8,
    binding_id: String,
    protocol_identity: String,
    codex_source_commit: String,
    codex_runtime_version: String,
    app_server_schema_sha256: String,
    codex_home: String,
    state: StateSecurityPolicy,
    environment: EnvironmentSecurityPolicy,
    request_policy: RequestSecurityPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateSecurityPolicy {
    isolation: StateIsolation,
    inherit_ambient_config: bool,
    cli_auth_credentials_store: CredentialStore,
    check_for_update_on_startup: bool,
    maximum_live_process_trees: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum StateIsolation {
    IsolatedBindingDirectory,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum CredentialStore {
    File,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnvironmentSecurityPolicy {
    inherit_ambient: bool,
    allowed_keys: Vec<String>,
    allowed_secret_keys: Vec<String>,
    maximum_entries: u8,
    maximum_key_bytes: u64,
    maximum_value_bytes: u64,
    maximum_total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RequestSecurityPolicy {
    thread_start: ThreadSecurityPolicy,
    thread_resume: ThreadSecurityPolicy,
    turn_start: TurnSecurityPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ThreadSecurityPolicy {
    approval_policy: ApprovalPolicy,
    sandbox: SandboxMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TurnSecurityPolicy {
    approval_policy: ApprovalPolicy,
    sandbox_policy: TurnSandboxPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TurnSandboxPolicy {
    r#type: SandboxMode,
    network_access: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ApprovalPolicy {
    Never,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SandboxMode {
    ReadOnly,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecurityPolicyPrerequisite(CodexSecurityPolicy);

impl SecurityPolicyPrerequisite {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
    ) -> Result<(), SecurityPolicyError> {
        validate_policy(&self.0, packet)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SecurityPolicyError {
    #[error("Codex security policy exceeds its byte limit")]
    PolicyBytes,
    #[error("Codex security policy is malformed or has an unknown field")]
    Malformed,
    #[error("Codex security policy identity does not match the packet")]
    Identity,
    #[error("Codex auth/state policy is not the pinned isolated policy")]
    State,
    #[error("Codex environment policy is not the exact cleared allowlist")]
    Environment,
    #[error("Codex request policy is not approval-never and read-only")]
    Request,
}

pub(crate) fn parse_security_policy(
    bytes: &[u8],
    packet: &MetadataValidatedPacket,
) -> Result<SecurityPolicyPrerequisite, SecurityPolicyError> {
    if bytes.len() > SECURITY_POLICY_BYTES {
        return Err(SecurityPolicyError::PolicyBytes);
    }
    let policy = serde_json::from_slice(bytes).map_err(|_| SecurityPolicyError::Malformed)?;
    validate_policy(&policy, packet)?;
    Ok(SecurityPolicyPrerequisite(policy))
}

fn validate_policy(
    policy: &CodexSecurityPolicy,
    packet: &MetadataValidatedPacket,
) -> Result<(), SecurityPolicyError> {
    let metadata = packet.metadata();
    if policy.schema_version != 1
        || policy.binding_id != metadata.binding_id
        || policy.protocol_identity != metadata.capability_ceiling.protocol_identity
        || policy.codex_source_commit != metadata.codex_source_commit
        || policy.codex_runtime_version != metadata.codex_runtime_version
        || policy.app_server_schema_sha256 != metadata.app_server_schema_sha256
        || policy.codex_home != packet.codex_home().as_os_str().to_string_lossy()
    {
        return Err(SecurityPolicyError::Identity);
    }

    if metadata.state_policy != StatePolicy::IsolatedBindingDirectory
        || policy.state.isolation != StateIsolation::IsolatedBindingDirectory
        || policy.state.inherit_ambient_config
        || policy.state.cli_auth_credentials_store != CredentialStore::File
        || policy.state.check_for_update_on_startup
        || policy.state.maximum_live_process_trees != 1
    {
        return Err(SecurityPolicyError::State);
    }

    if policy.environment.inherit_ambient
        || policy.environment.allowed_keys != ALLOWED_ENVIRONMENT_KEYS.map(str::to_owned)
        || !policy.environment.allowed_secret_keys.is_empty()
        || policy.environment.maximum_entries as usize != ALLOWED_ENVIRONMENT_KEYS.len()
        || policy.environment.maximum_entries as u64 > LIMITS.process.environment_entries
        || policy.environment.maximum_key_bytes != LIMITS.process.environment_key_bytes
        || policy.environment.maximum_value_bytes != LIMITS.process.environment_value_bytes
        || policy.environment.maximum_total_bytes != LIMITS.process.environment_bytes_total
    {
        return Err(SecurityPolicyError::Environment);
    }

    let thread = ThreadSecurityPolicy {
        approval_policy: ApprovalPolicy::Never,
        sandbox: SandboxMode::ReadOnly,
    };
    let turn = TurnSecurityPolicy {
        approval_policy: ApprovalPolicy::Never,
        sandbox_policy: TurnSandboxPolicy {
            r#type: SandboxMode::ReadOnly,
            network_access: SANDBOX_NETWORK_ACCESS,
        },
    };
    if policy.request_policy.thread_start != thread
        || policy.request_policy.thread_resume != thread
        || policy.request_policy.turn_start != turn
    {
        return Err(SecurityPolicyError::Request);
    }
    Ok(())
}
