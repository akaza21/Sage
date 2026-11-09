use super::*;

pub(super) const CODEX_ENVIRONMENT_KEYS: [&str; 4] = [
    "SAGE_CODEX_FIXTURE_PATH",
    "SAGE_CODEX_FIXTURE_SHA256",
    "SAGE_CODEX_FIXTURE_BYTES",
    "SAGE_CODEX_FIXTURE_CASE",
];
pub(super) const CLAUDE_ENVIRONMENT_KEYS: [&str; 4] = [
    "SAGE_CLAUDE_FIXTURE_PATH",
    "SAGE_CLAUDE_FIXTURE_SHA256",
    "SAGE_CLAUDE_FIXTURE_BYTES",
    "SAGE_CLAUDE_FIXTURE_CASE",
];
pub(super) const PI_ENVIRONMENT_KEYS: [&str; 4] = [
    "SAGE_PI_FIXTURE_PATH",
    "SAGE_PI_FIXTURE_SHA256",
    "SAGE_PI_FIXTURE_BYTES",
    "SAGE_PI_FIXTURE_CASE",
];
const ENVIRONMENT_KEYS: [[&str; 4]; 4] = [
    [
        "SAGE_CHARACTERIZATION_FIXTURE_PATH",
        "SAGE_CHARACTERIZATION_FIXTURE_SHA256",
        "SAGE_CHARACTERIZATION_FIXTURE_BYTES",
        "SAGE_CHARACTERIZATION_CASE",
    ],
    CODEX_ENVIRONMENT_KEYS,
    CLAUDE_ENVIRONMENT_KEYS,
    PI_ENVIRONMENT_KEYS,
];

pub(super) fn validate_inputs(inputs: [[Option<&OsStr>; 4]; 4], available: bool) -> io::Result<()> {
    let selected = inputs
        .iter()
        .filter(|values| values.iter().any(Option::is_some))
        .count();
    if selected > 1
        || (!available && selected != 0)
        || inputs
            .iter()
            .any(|values| values.iter().any(Option::is_some) && values.iter().any(Option::is_none))
    {
        return Err(invalid_profile());
    }
    Ok(())
}

pub(crate) fn reject_unavailable_fixture_environment_inputs() -> io::Result<()> {
    let values = ENVIRONMENT_KEYS.map(|keys| keys.map(std::env::var_os));
    validate_inputs(
        values
            .each_ref()
            .map(|values| values.each_ref().map(Option::as_deref)),
        cfg!(all(windows, feature = "acp-fixture", debug_assertions)),
    )?;
    Ok(())
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[derive(Debug, Clone)]
pub(crate) struct FixtureSelection {
    pub(crate) proof_root: PathBuf,
    pub(crate) profile_root: PathBuf,
    pub(crate) durable_dir: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) artifact_path: PathBuf,
    pub(crate) artifact_sha256: [u8; 32],
    pub(crate) artifact_bytes: u64,
    pub(crate) case: String,
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
impl FixtureSelection {
    pub(crate) fn launch_metadata(
        &self,
        binding_digest: [u8; 32],
    ) -> Result<
        (
            uuid::Uuid,
            crate::acp::process::ArtifactExpectation,
            crate::acp::process::BoundedOsArgs,
        ),
        crate::acp::bindings::adapter::AdapterRuntimeError,
    > {
        use crate::acp::{
            bindings::adapter::AdapterRuntimeError,
            process::{ArtifactExpectation, BoundedOsArgs},
        };
        let identity = |digest: [u8; 32]| {
            let mut bytes = [0; 16];
            bytes.copy_from_slice(&digest[..16]);
            uuid::Uuid::from_bytes(bytes)
        };
        let artifact = ArtifactExpectation::new(
            identity(self.artifact_sha256),
            self.artifact_path.clone(),
            self.artifact_sha256,
            self.artifact_bytes,
        )
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        let fixed_args = BoundedOsArgs::new(vec![
            "--case".into(),
            self.case.clone().into(),
            "--seed".into(),
            "0".into(),
        ])
        .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        Ok((identity(binding_digest), artifact, fixed_args))
    }

    pub(crate) fn revalidate(&self) -> io::Result<()> {
        require_plain_directory(&self.proof_root)?;
        if !self.profile_root.starts_with(&self.proof_root) || self.profile_root == self.proof_root
        {
            return Err(invalid_profile());
        }
        let mut suffix = self.proof_root.clone();
        for component in self
            .profile_root
            .strip_prefix(&self.proof_root)
            .map_err(|_| invalid_profile())?
            .components()
        {
            suffix.push(component);
            require_plain_directory(&suffix)?;
        }
        require_plain_directory(&self.profile_root)?;
        require_marker(&self.profile_root)?;
        require_plain_directory(&self.durable_dir)?;
        require_plain_directory(&self.project_root)?;
        if self.durable_dir.parent() != Some(self.profile_root.as_path())
            || overlaps(&self.project_root, &self.profile_root)
        {
            return Err(invalid_profile());
        }
        verify_fixture_artifact(
            &self.artifact_path,
            self.artifact_sha256,
            self.artifact_bytes,
        )
    }

    /// Claims this profile's launch of the fixture case. A case that resumes
    /// its session after an app restart gets one more launch for that run.
    pub(crate) fn consume_one_shot(&self) -> io::Result<()> {
        self.revalidate()?;
        claim_launch(
            &self.durable_dir,
            crate::acp::fixture::native_case_spans_restart(&self.case),
        )
    }
}

#[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
const LAUNCH_RECEIPTS: [(&str, &[u8]); 2] = [
    (
        "codex-fixture-launch-once-v1",
        b"sage:cp14:codex-fixture-launch-once:v1\n",
    ),
    (
        "fixture-relaunch-once-v1",
        b"sage:fixture-relaunch-once:v1\n",
    ),
];

/// Writes the first launch receipt not yet in `durable_dir`, so each launch
/// a profile allows is claimed once and survives the process that took it.
#[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
pub(super) fn claim_launch(durable_dir: &Path, spans_restart: bool) -> io::Result<()> {
    use std::io::Write;

    let allowed = if spans_restart { 2 } else { 1 };
    for (name, body) in &LAUNCH_RECEIPTS[..allowed] {
        let mut file = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(durable_dir.join(name))
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        file.write_all(body)?;
        return file.sync_all();
    }
    Err(io::ErrorKind::AlreadyExists.into())
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) fn resolve_selection(
    profile: &LaunchProfile,
    values: [Option<&OsStr>; 4],
    proof_root: &Path,
    protocol: &str,
) -> io::Result<Option<FixtureSelection>> {
    let [artifact_path, artifact_sha256, artifact_bytes, case] = values;
    let inputs = [
        artifact_path.is_some(),
        artifact_sha256.is_some(),
        artifact_bytes.is_some(),
        case.is_some(),
    ];
    if inputs.iter().all(|present| !present) {
        return Ok(None);
    }
    if !inputs.iter().all(|present| *present)
        || !profile.is_disposable()
        || profile.project_root().is_none()
        || !case
            .and_then(OsStr::to_str)
            .is_some_and(|case| crate::acp::fixture::native_case_matches(case, protocol))
    {
        return Err(invalid_profile());
    }
    require_plain_directory(proof_root)?;
    let proof_root = proof_root.canonicalize()?;
    let profile_root = profile.disposable_root().ok_or_else(invalid_profile)?;
    if !profile_root.starts_with(&proof_root) || profile_root == proof_root {
        return Err(invalid_profile());
    }
    let mut suffix = proof_root.clone();
    for component in profile_root
        .strip_prefix(&proof_root)
        .map_err(|_| invalid_profile())?
        .components()
    {
        suffix.push(component);
        require_plain_directory(&suffix)?;
    }
    require_marker(profile_root)?;
    let artifact_path = PathBuf::from(artifact_path.expect("complete selector"));
    let artifact_sha256 = parse_sha256(artifact_sha256.expect("complete selector"))?;
    let artifact_bytes = artifact_bytes
        .expect("complete selector")
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0 && *value <= 512 * 1024 * 1024)
        .ok_or_else(invalid_profile)?;
    if [
        profile_root,
        profile.durable_dir(),
        profile.project_root().ok_or_else(invalid_profile)?,
        &artifact_path,
    ]
    .iter()
    .any(|path| path.to_str().is_none())
    {
        return Err(invalid_profile());
    }
    verify_fixture_artifact(&artifact_path, artifact_sha256, artifact_bytes)?;
    let selection = FixtureSelection {
        case: case
            .and_then(OsStr::to_str)
            .ok_or_else(invalid_profile)?
            .to_owned(),
        proof_root,
        profile_root: profile_root.to_path_buf(),
        durable_dir: profile.durable_dir().to_path_buf(),
        project_root: profile
            .project_root()
            .ok_or_else(invalid_profile)?
            .to_path_buf(),
        artifact_path: artifact_path.canonicalize()?,
        artifact_sha256,
        artifact_bytes,
    };
    selection.revalidate()?;
    Ok(Some(selection))
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
fn verify_fixture_artifact(path: &Path, sha256: [u8; 32], bytes: u64) -> io::Result<()> {
    use sha2::{Digest, Sha256};

    if !is_local_absolute(path) || path.extension() != Some(OsStr::new("exe")) {
        return Err(invalid_profile());
    }
    let mut prefix = PathBuf::new();
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(invalid_profile());
        }
        prefix.push(component);
        if prefix.is_dir() {
            require_plain_directory(&prefix)?;
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_reparse(&metadata) || metadata.len() != bytes {
        return Err(invalid_profile());
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    let mut observed_bytes = 0_u64;
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        observed_bytes = observed_bytes
            .checked_add(read as u64)
            .ok_or_else(invalid_profile)?;
        if observed_bytes > bytes {
            return Err(invalid_profile());
        }
        digest.update(&chunk[..read]);
    }
    if observed_bytes != bytes || <[u8; 32]>::from(digest.finalize()) != sha256 {
        return Err(invalid_profile());
    }
    Ok(())
}
