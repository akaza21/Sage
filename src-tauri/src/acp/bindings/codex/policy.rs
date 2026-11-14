use serde::Deserialize;

use sage_core::limits::LIMITS;

use super::manifest::{MetadataValidatedPacket, StatePolicy};

pub(super) const SECURITY_POLICY_BYTES: usize = 16 * 1024;
pub(super) const APPROVAL_POLICY_NEVER: &str = "never";
pub(super) const SANDBOX_READ_ONLY: &str = "read-only";
pub(super) const SANDBOX_NETWORK_ACCESS: bool = false;
pub(super) const CONFIG_SHA256: &str =
    "cda4ac431cf5f9c615c36513e2831d159bdfc0b3d106cb475012d1677748afd3";
pub(super) const STATE_LOGICAL_BYTES_MAX: u64 = 536_870_912;
pub(super) const STATE_ALLOCATED_BYTES_MAX: u64 = 536_870_912;
pub(super) const STATE_REGULAR_FILES_MAX: u64 = 2_048;
pub(super) const STATE_DIRECTORIES_MAX: u64 = 512;
pub(super) const STATE_DEPTH_MAX: u8 = 8;
pub(super) const STATE_SINGLE_FILE_BYTES_MAX: u64 = 134_217_728;
pub(super) const STATE_CREDENTIAL_FILE_BYTES_MAX: u64 = 262_144;
pub(super) const STATE_RELATIVE_PATH_BYTES_MAX: u64 = 16_384;
pub(super) const STATE_NATIVE_PATH_BYTES_MAX: u64 = 32_768;
pub(super) const TEMPORARY_RESIDUE_RETENTION_HOURS: u16 = 24;

const ALLOWED_ENVIRONMENT_KEYS: [&str; 6] =
    ["CODEX_HOME", "PATH", "SystemRoot", "TEMP", "TMP", "WINDIR"];

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
    auth: AuthSecurityPolicy,
    storage: StorageSecurityPolicy,
    network: NetworkSecurityPolicy,
    consent: ConsentSecurityPolicy,
    environment: EnvironmentSecurityPolicy,
    request_policy: RequestSecurityPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StateSecurityPolicy {
    isolation: StateIsolation,
    inherit_ambient_config: bool,
    config_relative_path: String,
    config_sha256: String,
    cli_auth_credentials_store: CredentialStore,
    check_for_update_on_startup: bool,
    maximum_live_process_trees: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthSecurityPolicy {
    login_mode: LoginMode,
    automatic_login_start: bool,
    sage_stores_credentials: bool,
    logout_requires_account_updated_null: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LoginMode {
    ChatgptBrowser,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StorageSecurityPolicy {
    maximum_logical_bytes: u64,
    maximum_allocated_bytes: u64,
    maximum_regular_files: u64,
    maximum_directories: u64,
    maximum_depth: u8,
    maximum_single_file_bytes: u64,
    maximum_credential_file_bytes: u64,
    maximum_relative_path_bytes: u64,
    maximum_native_path_bytes: u64,
    temporary_residue_retention_hours: u16,
    automatic_credential_deletion: bool,
    automatic_session_deletion: bool,
    preserve_binding_home_on_update: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NetworkSecurityPolicy {
    provider: Provider,
    custom_endpoints_allowed: bool,
    proxy: ProxyPolicy,
    tls_trust: TlsTrust,
    custom_tls_roots_allowed: bool,
    offline: bool,
    analytics_enabled: bool,
    otel_log_exporter: TelemetryExporter,
    otel_trace_exporter: TelemetryExporter,
    otel_metrics_exporter: TelemetryExporter,
    log_user_prompt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Provider {
    BuiltInOpenAi,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum ProxyPolicy {
    None,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TlsTrust {
    PlatformRoots,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum TelemetryExporter {
    None,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConsentSecurityPolicy {
    login_requires_explicit_user_action: bool,
    delete_data_requires_explicit_consent: bool,
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
#[serde(rename_all = "kebab-case")]
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

    pub(super) fn config_sha256(&self) -> &str {
        &self.0.state.config_sha256
    }

    pub(super) fn codex_home(&self) -> &str {
        &self.0.codex_home
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
    #[error("Codex authentication policy is not the pinned managed ChatGPT policy")]
    Auth,
    #[error("Codex storage policy is not the pinned beta stop policy")]
    Storage,
    #[error("Codex endpoint, proxy, TLS, offline, or telemetry policy is not pinned")]
    Network,
    #[error("Codex login or deletion consent policy is not pinned")]
    Consent,
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
        || policy.state.config_relative_path != "config.toml"
        || policy.state.config_sha256 != CONFIG_SHA256
        || policy.state.cli_auth_credentials_store != CredentialStore::File
        || policy.state.check_for_update_on_startup
        || policy.state.maximum_live_process_trees != 1
    {
        return Err(SecurityPolicyError::State);
    }

    if policy.auth.login_mode != LoginMode::ChatgptBrowser
        || policy.auth.automatic_login_start
        || policy.auth.sage_stores_credentials
        || !policy.auth.logout_requires_account_updated_null
    {
        return Err(SecurityPolicyError::Auth);
    }

    if policy.storage.maximum_logical_bytes != STATE_LOGICAL_BYTES_MAX
        || policy.storage.maximum_allocated_bytes != STATE_ALLOCATED_BYTES_MAX
        || policy.storage.maximum_regular_files != STATE_REGULAR_FILES_MAX
        || policy.storage.maximum_directories != STATE_DIRECTORIES_MAX
        || policy.storage.maximum_depth != STATE_DEPTH_MAX
        || policy.storage.maximum_single_file_bytes != STATE_SINGLE_FILE_BYTES_MAX
        || policy.storage.maximum_credential_file_bytes != STATE_CREDENTIAL_FILE_BYTES_MAX
        || policy.storage.maximum_relative_path_bytes != STATE_RELATIVE_PATH_BYTES_MAX
        || policy.storage.maximum_native_path_bytes != STATE_NATIVE_PATH_BYTES_MAX
        || policy.storage.temporary_residue_retention_hours != TEMPORARY_RESIDUE_RETENTION_HOURS
        || policy.storage.automatic_credential_deletion
        || policy.storage.automatic_session_deletion
        || !policy.storage.preserve_binding_home_on_update
    {
        return Err(SecurityPolicyError::Storage);
    }

    if policy.network.provider != Provider::BuiltInOpenAi
        || policy.network.custom_endpoints_allowed
        || policy.network.proxy != ProxyPolicy::None
        || policy.network.tls_trust != TlsTrust::PlatformRoots
        || policy.network.custom_tls_roots_allowed
        || policy.network.offline
        || policy.network.analytics_enabled
        || policy.network.otel_log_exporter != TelemetryExporter::None
        || policy.network.otel_trace_exporter != TelemetryExporter::None
        || policy.network.otel_metrics_exporter != TelemetryExporter::None
        || policy.network.log_user_prompt
    {
        return Err(SecurityPolicyError::Network);
    }

    if !policy.consent.login_requires_explicit_user_action
        || !policy.consent.delete_data_requires_explicit_consent
    {
        return Err(SecurityPolicyError::Consent);
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
