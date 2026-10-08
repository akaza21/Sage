use super::*;

fn codex_fixture_environment_values() -> [Option<std::ffi::OsString>; 4] {
    [
        std::env::var_os("SAGE_CODEX_FIXTURE_PATH"),
        std::env::var_os("SAGE_CODEX_FIXTURE_SHA256"),
        std::env::var_os("SAGE_CODEX_FIXTURE_BYTES"),
        std::env::var_os("SAGE_CODEX_FIXTURE_CASE"),
    ]
}

pub(crate) fn reject_unavailable_codex_fixture_environment_inputs() -> io::Result<()> {
    let inputs = codex_fixture_environment_values();
    reject_unavailable_codex_fixture_inputs(inputs.each_ref().map(Option::as_deref))
}

pub(crate) fn reject_unavailable_codex_fixture_inputs(
    inputs: [Option<&OsStr>; 4],
) -> io::Result<()> {
    if cfg!(all(windows, feature = "acp-fixture", debug_assertions))
        || inputs.iter().all(|input| input.is_none())
    {
        Ok(())
    } else {
        Err(invalid_profile())
    }
}

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) fn resolve_desktop_fixture_mode_from_environment(
    profile: &LaunchProfile,
) -> io::Result<DesktopFixtureMode> {
    let codex = codex_fixture_environment_values();
    resolve_desktop_fixture_mode(
        profile,
        [
            std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_PATH").as_deref(),
            std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_SHA256").as_deref(),
            std::env::var_os("SAGE_CHARACTERIZATION_FIXTURE_BYTES").as_deref(),
            std::env::var_os("SAGE_CHARACTERIZATION_CASE").as_deref(),
        ],
        codex.each_ref().map(Option::as_deref),
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has checkout parent")
            .join("work/cp-native-proof"),
    )
}

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) enum DesktopFixtureMode {
    Ordinary,
    GenericCharacterization(CharacterizationBinding),
    #[cfg(debug_assertions)]
    CodexFixture(CodexFixtureSelection),
}

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) fn resolve_desktop_fixture_mode(
    profile: &LaunchProfile,
    generic: [Option<&OsStr>; 4],
    codex: [Option<&OsStr>; 4],
    proof_root: &Path,
) -> io::Result<DesktopFixtureMode> {
    let generic =
        resolve_characterization_binding(profile, generic[0], generic[1], generic[2], generic[3])?;
    #[cfg(debug_assertions)]
    let codex = resolve_codex_fixture_selection(
        profile, codex[0], codex[1], codex[2], codex[3], proof_root,
    )?;
    #[cfg(not(debug_assertions))]
    {
        let _ = proof_root;
        if codex.iter().any(Option::is_some) {
            return Err(invalid_profile());
        }
    }
    match generic {
        Some(binding) => {
            #[cfg(debug_assertions)]
            if codex.is_some() {
                return Err(invalid_profile());
            }
            Ok(DesktopFixtureMode::GenericCharacterization(binding))
        }
        None => {
            #[cfg(debug_assertions)]
            if let Some(selection) = codex {
                return Ok(DesktopFixtureMode::CodexFixture(selection));
            }
            Ok(DesktopFixtureMode::Ordinary)
        }
    }
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[derive(Debug, Clone)]
pub(crate) struct CodexFixtureSelection {
    pub(crate) proof_root: PathBuf,
    pub(crate) profile_root: PathBuf,
    pub(crate) durable_dir: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) artifact_path: PathBuf,
    pub(crate) artifact_sha256: [u8; 32],
    pub(crate) artifact_bytes: u64,
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
impl CodexFixtureSelection {
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
        verify_codex_fixture_artifact(
            &self.artifact_path,
            self.artifact_sha256,
            self.artifact_bytes,
        )
    }

    pub(crate) fn consume_one_shot(&self) -> io::Result<()> {
        use std::io::Write;

        self.revalidate()?;
        let receipt = self.durable_dir.join("codex-fixture-launch-once-v1");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(receipt)?;
        file.write_all(b"sage:cp14:codex-fixture-launch-once:v1\n")?;
        file.sync_all()
    }
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) fn resolve_codex_fixture_selection(
    profile: &LaunchProfile,
    artifact_path: Option<&OsStr>,
    artifact_sha256: Option<&OsStr>,
    artifact_bytes: Option<&OsStr>,
    case: Option<&OsStr>,
    proof_root: &Path,
) -> io::Result<Option<CodexFixtureSelection>> {
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
        || case != Some(OsStr::new("codex-steer-lost-result"))
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
    verify_codex_fixture_artifact(&artifact_path, artifact_sha256, artifact_bytes)?;
    let selection = CodexFixtureSelection {
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
fn verify_codex_fixture_artifact(path: &Path, sha256: [u8; 32], bytes: u64) -> io::Result<()> {
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
    loop {
        let read = file.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    if <[u8; 32]>::from(digest.finalize()) != sha256 {
        return Err(invalid_profile());
    }
    Ok(())
}
