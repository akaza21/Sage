use std::path::{Component, Path};

pub(crate) const OMP_VERSION: &str = "17.2.3";
pub(crate) const OMP_SOURCE_COMMIT: &str = "8baa3300bc23c721fc80c0307f0b7a8218be8fcb";
pub(crate) const OMP_PACKAGE: &str = "@oh-my-pi/pi-coding-agent";
pub(crate) const OMP_LICENSE: &str = "MIT";
pub(crate) const OMP_BUN_MINIMUM: &str = "1.3.14";
const OMP_ARGS: [&str; 1] = ["acp"];
const MAX_NATIVE_ADDONS: usize = 32;
const MAX_ARTIFACT_BYTES: u64 = 1_073_741_824;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OmpPlatform {
    WindowsX64,
    LinuxX64,
    LinuxArm64,
    MacOsX64,
    MacOsArm64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtifactIdentity {
    pub relative_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CandidateManifest {
    pub platform: OmpPlatform,
    pub package_name: String,
    pub package_version: String,
    pub source_commit: String,
    pub license: String,
    pub acp_protocol_version: String,
    pub bun_version: String,
    pub entrypoint: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub native_addons: Vec<ArtifactIdentity>,
    pub fixed_args: Vec<String>,
    pub isolated_state: bool,
    pub isolated_auth: bool,
    pub ambient_extensions_disabled: bool,
    pub ambient_configuration_disabled: bool,
    pub dynamic_downloads_disabled: bool,
    pub browser_subprocess_lsp_dap_disabled: bool,
    pub offline_and_telemetry_reviewed: bool,
    pub read_only_authority_verified: bool,
    pub native_acp_ndjson_verified: bool,
    pub disconnect_teardown_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CuratedOmpBinding {
    pub platform: OmpPlatform,
    pub acp_protocol_version: String,
    pub bun_version: String,
    pub entrypoint: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub native_addons: Vec<ArtifactIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResearchBinding {
    capabilities: OmpBindingCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OmpBindingCapabilities {
    initialize: bool,
    new_session: bool,
    prompt: bool,
    cancel: bool,
    load_session_ceiling: bool,
    resume_session: bool,
    close_session: bool,
    multi_root: bool,
    write_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum OmpBindingError {
    #[error("curated OMP metadata is malformed: {0}")]
    InvalidCuratedMetadata(&'static str),
    #[error("OMP artifact metadata does not match the curated record: {0}")]
    IdentityMismatch(&'static str),
    #[error("OMP prompt authority is not proven read-only")]
    ReadOnlyUnproven,
    #[error("OMP's immutable Bun/native-addon graph is research-only and not admitted")]
    ArtifactGraphNotAdmitted,
}

impl CuratedOmpBinding {
    pub(crate) fn validate_candidate(
        &self,
        candidate: &CandidateManifest,
    ) -> Result<ResearchBinding, OmpBindingError> {
        validate_version(&self.acp_protocol_version, "ACP protocol version")?;
        validate_version(&self.bun_version, "Bun version")?;
        if parse_version(&self.bun_version) < Some((1, 3, 14)) {
            return Err(OmpBindingError::InvalidCuratedMetadata("Bun version"));
        }
        validate_artifact_set(&self.entrypoint, &self.runtime, &self.native_addons)?;
        macro_rules! exact {
            ($condition:expr, $field:literal) => {
                if !$condition {
                    return Err(OmpBindingError::IdentityMismatch($field));
                }
            };
        }
        exact!(candidate.platform == self.platform, "platform");
        exact!(candidate.package_name == OMP_PACKAGE, "package name");
        exact!(candidate.package_version == OMP_VERSION, "package version");
        exact!(
            candidate.source_commit == OMP_SOURCE_COMMIT,
            "source commit"
        );
        exact!(candidate.license == OMP_LICENSE, "license");
        exact!(
            candidate.acp_protocol_version == self.acp_protocol_version,
            "ACP protocol"
        );
        exact!(candidate.bun_version == self.bun_version, "Bun version");
        exact!(
            candidate.entrypoint == self.entrypoint,
            "entrypoint identity"
        );
        exact!(candidate.runtime == self.runtime, "runtime identity");
        exact!(
            candidate.native_addons == self.native_addons,
            "native addon identities"
        );
        exact!(
            candidate.fixed_args == OMP_ARGS.map(str::to_owned),
            "fixed arguments"
        );
        exact!(candidate.isolated_state, "state isolation");
        exact!(candidate.isolated_auth, "auth isolation");
        exact!(candidate.ambient_extensions_disabled, "extension isolation");
        exact!(
            candidate.ambient_configuration_disabled,
            "configuration isolation"
        );
        exact!(candidate.dynamic_downloads_disabled, "dynamic downloads");
        exact!(
            candidate.browser_subprocess_lsp_dap_disabled,
            "broad process authority"
        );
        exact!(
            candidate.offline_and_telemetry_reviewed,
            "offline and telemetry policy"
        );
        exact!(candidate.native_acp_ndjson_verified, "native ACP NDJSON");
        exact!(
            candidate.disconnect_teardown_verified,
            "disconnect teardown"
        );
        if !candidate.read_only_authority_verified {
            return Err(OmpBindingError::ReadOnlyUnproven);
        }
        Ok(ResearchBinding {
            capabilities: OmpBindingCapabilities::read_only_native_acp(),
        })
    }
}

impl ResearchBinding {
    pub(crate) const fn capabilities(&self) -> OmpBindingCapabilities {
        self.capabilities
    }

    pub(crate) fn require_launch_admission(&self) -> Result<(), OmpBindingError> {
        Err(OmpBindingError::ArtifactGraphNotAdmitted)
    }
}

impl OmpBindingCapabilities {
    const fn read_only_native_acp() -> Self {
        Self {
            initialize: true,
            new_session: true,
            prompt: true,
            cancel: true,
            load_session_ceiling: true,
            resume_session: false,
            close_session: false,
            multi_root: false,
            write_tools: false,
        }
    }

    pub(crate) const fn initialize(self) -> bool {
        self.initialize
    }
    pub(crate) const fn new_session(self) -> bool {
        self.new_session
    }
    pub(crate) const fn prompt(self) -> bool {
        self.prompt
    }
    pub(crate) const fn cancel(self) -> bool {
        self.cancel
    }
    pub(crate) const fn load_session_ceiling(self) -> bool {
        self.load_session_ceiling
    }
    pub(crate) const fn resume_session(self) -> bool {
        self.resume_session
    }
    pub(crate) const fn close_session(self) -> bool {
        self.close_session
    }
    pub(crate) const fn multi_root(self) -> bool {
        self.multi_root
    }
    pub(crate) const fn write_tools(self) -> bool {
        self.write_tools
    }
}

fn validate_artifact_set(
    entrypoint: &ArtifactIdentity,
    runtime: &ArtifactIdentity,
    addons: &[ArtifactIdentity],
) -> Result<(), OmpBindingError> {
    if addons.len() > MAX_NATIVE_ADDONS {
        return Err(OmpBindingError::InvalidCuratedMetadata(
            "native addon count",
        ));
    }
    let mut total = 0_u64;
    for artifact in std::iter::once(entrypoint)
        .chain(std::iter::once(runtime))
        .chain(addons)
    {
        validate_identity(artifact)?;
        total = total
            .checked_add(artifact.bytes)
            .ok_or(OmpBindingError::InvalidCuratedMetadata("artifact bytes"))?;
        if total > MAX_ARTIFACT_BYTES {
            return Err(OmpBindingError::InvalidCuratedMetadata("artifact bytes"));
        }
    }
    let mut paths = addons
        .iter()
        .map(|artifact| &artifact.relative_path)
        .collect::<Vec<_>>();
    paths.sort_unstable();
    if paths.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(OmpBindingError::InvalidCuratedMetadata(
            "duplicate addon path",
        ));
    }
    Ok(())
}

fn validate_identity(identity: &ArtifactIdentity) -> Result<(), OmpBindingError> {
    if identity.bytes == 0
        || identity.sha256.len() != 64
        || !identity
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(OmpBindingError::InvalidCuratedMetadata("artifact identity"));
    }
    let path = Path::new(&identity.relative_path);
    if identity.relative_path.is_empty()
        || path.is_absolute()
        || identity.relative_path.contains(['\\', ':', '\0'])
        || identity
            .relative_path
            .split('/')
            .any(|part| part.is_empty() || matches!(part, "." | ".."))
        || path.components().any(|part| {
            matches!(
                part,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(OmpBindingError::InvalidCuratedMetadata(
            "canonical relative artifact path",
        ));
    }
    Ok(())
}

fn validate_version(value: &str, field: &'static str) -> Result<(), OmpBindingError> {
    if value.is_empty() || value.len() > 64 || value.contains(['\0', '/', '\\']) {
        return Err(OmpBindingError::InvalidCuratedMetadata(field));
    }
    Ok(())
}

fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}
