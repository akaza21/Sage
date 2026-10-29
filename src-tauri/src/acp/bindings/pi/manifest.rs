use std::path::{Component, Path};

pub const PI_VERSION: &str = "0.83.0";
pub const PI_SOURCE_COMMIT: &str = "845d6ff1f6643aba440341cce877ce1c43ebbc39";
pub const PI_PACKAGE: &str = "@earendil-works/pi-coding-agent";
pub const PI_LICENSE: &str = "MIT";
pub const PI_NODE_MINIMUM: &str = "22.19.0";
pub const PI_RPC_ARGS: [&str; 2] = ["--mode", "rpc"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiPlatform {
    WindowsX64,
    LinuxX64,
    LinuxArm64,
    MacOsX64,
    MacOsArm64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactIdentity {
    pub relative_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactCandidate {
    pub platform: PiPlatform,
    pub package_name: String,
    pub package_version: String,
    pub source_commit: String,
    pub license: String,
    pub node_version: String,
    pub entrypoint: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub fixed_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedPiBinding {
    pub platform: PiPlatform,
    pub entrypoint: ArtifactIdentity,
    pub runtime: ArtifactIdentity,
    pub node_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchPiBinding {
    capabilities: PiBindingCapabilities,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LaunchAdmission {
    capabilities: PiBindingCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PiBindingCapabilities {
    initialize: bool,
    model_discovery: bool,
    new_session: bool,
    prompt: bool,
    abort: bool,
    state: bool,
    messages: bool,
    entries: bool,
    resume: bool,
    load: bool,
    close: bool,
    multi_root: bool,
    fork_or_clone: bool,
    write_tools: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PiBindingError {
    #[error("curated Pi metadata is malformed: {0}")]
    InvalidCuratedMetadata(&'static str),
    #[error("Pi artifact metadata does not match the curated admission record: {0}")]
    IdentityMismatch(&'static str),
    #[error("Pi runtime identity and read-only authority remain research-only")]
    ArtifactGraphNotAdmitted,
}

impl CuratedPiBinding {
    pub fn validate_candidate(
        &self,
        candidate: &ArtifactCandidate,
    ) -> Result<ResearchPiBinding, PiBindingError> {
        validate_identity(&self.entrypoint)?;
        validate_identity(&self.runtime)?;
        if self.node_version.is_empty()
            || self.node_version.len() > 64
            || parse_version(&self.node_version) < Some((22, 19, 0))
        {
            return Err(PiBindingError::InvalidCuratedMetadata("node version"));
        }
        macro_rules! exact {
            ($condition:expr, $field:literal) => {
                if !$condition {
                    return Err(PiBindingError::IdentityMismatch($field));
                }
            };
        }
        exact!(candidate.platform == self.platform, "platform");
        exact!(candidate.package_name == PI_PACKAGE, "package name");
        exact!(candidate.package_version == PI_VERSION, "package version");
        exact!(candidate.source_commit == PI_SOURCE_COMMIT, "source commit");
        exact!(candidate.license == PI_LICENSE, "license");
        exact!(
            candidate.node_version == self.node_version,
            "runtime version"
        );
        exact!(
            candidate.entrypoint == self.entrypoint,
            "entrypoint identity"
        );
        exact!(candidate.runtime == self.runtime, "runtime identity");
        exact!(
            candidate.fixed_args == PI_RPC_ARGS.map(str::to_owned),
            "fixed arguments"
        );
        Ok(ResearchPiBinding {
            capabilities: PiBindingCapabilities::research_rpc(),
        })
    }
}

impl ResearchPiBinding {
    pub const fn capabilities(&self) -> PiBindingCapabilities {
        self.capabilities
    }

    pub fn require_launch_admission(&self) -> Result<(), PiBindingError> {
        Err(PiBindingError::ArtifactGraphNotAdmitted)
    }
}

impl LaunchAdmission {
    /// The read-only RPC authority shared by the fixture and the curated
    /// native launch (prompt and abort, no write tools).
    pub(super) const fn read_only() -> Self {
        Self {
            capabilities: PiBindingCapabilities::read_only_rpc(),
        }
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
    pub(super) const fn fixture() -> Self {
        Self::read_only()
    }

    pub(super) const fn capabilities(self) -> PiBindingCapabilities {
        self.capabilities
    }
}

impl PiBindingCapabilities {
    const fn research_rpc() -> Self {
        let mut capabilities = Self::read_only_rpc();
        capabilities.prompt = false;
        capabilities.abort = false;
        capabilities
    }

    const fn read_only_rpc() -> Self {
        Self {
            initialize: true,
            model_discovery: true,
            new_session: true,
            prompt: true,
            abort: true,
            state: true,
            messages: true,
            entries: true,
            resume: false,
            load: false,
            close: false,
            multi_root: false,
            fork_or_clone: false,
            write_tools: false,
        }
    }

    pub const fn initialize(self) -> bool {
        self.initialize
    }

    pub const fn model_discovery(self) -> bool {
        self.model_discovery
    }

    pub const fn new_session(self) -> bool {
        self.new_session
    }

    pub const fn prompt(self) -> bool {
        self.prompt
    }

    pub const fn abort(self) -> bool {
        self.abort
    }

    pub const fn state(self) -> bool {
        self.state
    }

    pub const fn messages(self) -> bool {
        self.messages
    }

    pub const fn entries(self) -> bool {
        self.entries
    }

    pub const fn resume(self) -> bool {
        self.resume
    }

    pub const fn load(self) -> bool {
        self.load
    }

    pub const fn close(self) -> bool {
        self.close
    }

    pub const fn multi_root(self) -> bool {
        self.multi_root
    }

    pub const fn fork_or_clone(self) -> bool {
        self.fork_or_clone
    }

    pub const fn write_tools(self) -> bool {
        self.write_tools
    }
}

pub(super) fn parse_version(value: &str) -> Option<(u64, u64, u64)> {
    let mut parts = value.split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

fn validate_identity(identity: &ArtifactIdentity) -> Result<(), PiBindingError> {
    if identity.bytes == 0 {
        return Err(PiBindingError::InvalidCuratedMetadata("artifact size"));
    }
    if identity.sha256.len() != 64
        || !identity
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PiBindingError::InvalidCuratedMetadata("sha256"));
    }
    let path = Path::new(&identity.relative_path);
    let segments_are_canonical = identity
        .relative_path
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if identity.relative_path.is_empty()
        || path.is_absolute()
        || identity.relative_path.contains('\\')
        || identity.relative_path.contains(':')
        || !segments_are_canonical
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
        return Err(PiBindingError::InvalidCuratedMetadata(
            "canonical relative artifact path",
        ));
    }
    Ok(())
}
