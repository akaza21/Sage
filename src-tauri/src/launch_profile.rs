use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
};

#[cfg(all(windows, feature = "acp-fixture"))]
use crate::acp::service::{
    CharacterizationArtifact, CharacterizationBinding, CharacterizationCase,
};

pub(crate) const PROFILE_MARKER_NAME: &str = ".sage-disposable-profile-v1";
pub(crate) const PROFILE_MARKER_BYTES: &[u8] = b"sage-disposable-profile-v1\n";

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
mod desktop_claude_fixture;
mod desktop_codex_fixture;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
mod desktop_pi_fixture;
mod fixture_selector;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) use fixture_selector::FixtureSelection;
pub(crate) use fixture_selector::reject_unavailable_fixture_environment_inputs;
#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(crate) type CodexFixtureSelection = FixtureSelection;
#[cfg(test)]
mod fixture_selector_tests;
#[cfg(all(test, windows, feature = "acp-fixture", debug_assertions))]
pub(crate) use desktop_codex_fixture::resolve_codex_fixture_selection;
#[cfg(all(test, windows, feature = "acp-fixture", debug_assertions))]
use desktop_codex_fixture::resolve_desktop_fixture_mode;
#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) use desktop_codex_fixture::{
    DesktopFixtureMode, resolve_desktop_fixture_mode_from_environment,
};

#[cfg(all(test, not(target_os = "macos")))]
#[path = "launch_profile/browser_tests.rs"]
mod browser_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaunchProfile {
    Normal {
        durable_dir: PathBuf,
    },
    Disposable {
        durable_dir: PathBuf,
        webview_dir: PathBuf,
        project_root: Option<PathBuf>,
    },
}

impl LaunchProfile {
    pub(crate) fn durable_dir(&self) -> &Path {
        match self {
            Self::Normal { durable_dir } | Self::Disposable { durable_dir, .. } => durable_dir,
        }
    }

    pub(crate) fn webview_dir(&self) -> Option<&Path> {
        match self {
            Self::Normal { .. } => None,
            Self::Disposable { webview_dir, .. } => Some(webview_dir),
        }
    }

    pub(crate) fn project_root(&self) -> Option<&Path> {
        match self {
            Self::Normal { .. } => None,
            Self::Disposable { project_root, .. } => project_root.as_deref(),
        }
    }

    pub(crate) fn is_disposable(&self) -> bool {
        matches!(self, Self::Disposable { .. })
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn disposable_root(&self) -> Option<&Path> {
        match self {
            Self::Normal { .. } => None,
            Self::Disposable { webview_dir, .. } => webview_dir.parent(),
        }
    }
}

pub(crate) fn resolve_launch_profile(
    configured_root: Option<&OsStr>,
    configured_project_root: Option<&OsStr>,
    normal_durable_dir: PathBuf,
    normal_webview_dir: PathBuf,
) -> io::Result<LaunchProfile> {
    let Some(configured_root) = configured_root else {
        if configured_project_root.is_some() {
            return Err(invalid_profile());
        }
        return Ok(LaunchProfile::Normal {
            durable_dir: normal_durable_dir,
        });
    };
    let configured_root = Path::new(configured_root);
    if cfg!(target_os = "macos") || !is_local_absolute(configured_root) {
        return Err(invalid_profile());
    }
    require_plain_directory(configured_root)?;
    let root = configured_root.canonicalize()?;
    let normal_durable = normalize_without_creating(&normal_durable_dir)?;
    let normal_webview = normalize_without_creating(&normal_webview_dir)?;
    if overlaps(&root, &normal_durable) || overlaps(&root, &normal_webview) {
        return Err(invalid_profile());
    }
    require_marker(&root)?;
    let project_root = configured_project_root
        .map(Path::new)
        .map(|project| {
            if !is_local_absolute(project) {
                return Err(invalid_profile());
            }
            require_plain_directory(project)?;
            let project = project.canonicalize()?;
            if overlaps(&project, &root)
                || overlaps(&project, &normal_durable)
                || overlaps(&project, &normal_webview)
            {
                return Err(invalid_profile());
            }
            Ok(project)
        })
        .transpose()?;
    require_profile_inventory(&root)?;
    let durable_dir = ensure_plain_child(&root, "durable")?;
    let webview_dir = ensure_plain_child(&root, "webview")?;
    require_profile_inventory(&root)?;
    Ok(LaunchProfile::Disposable {
        durable_dir,
        webview_dir,
        project_root,
    })
}

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) fn resolve_characterization_binding(
    profile: &LaunchProfile,
    artifact_path: Option<&OsStr>,
    artifact_sha256: Option<&OsStr>,
    artifact_bytes: Option<&OsStr>,
    case: Option<&OsStr>,
) -> io::Result<Option<CharacterizationBinding>> {
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
    {
        return Err(invalid_profile());
    }
    let artifact_path = PathBuf::from(artifact_path.expect("complete selector"));
    if !is_local_absolute(&artifact_path) {
        return Err(invalid_profile());
    }
    let sha256 = parse_sha256(artifact_sha256.expect("complete selector"))?;
    let bytes = artifact_bytes
        .expect("complete selector")
        .to_str()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(invalid_profile)?;
    let case = case
        .expect("complete selector")
        .to_str()
        .ok_or_else(invalid_profile)
        .and_then(|value| CharacterizationCase::parse(value).map_err(|_| invalid_profile()))?;
    let artifact = CharacterizationArtifact::new(artifact_path, sha256, bytes)
        .map_err(|_| invalid_profile())?;
    CharacterizationBinding::new(
        artifact,
        case,
        profile.disposable_root().ok_or_else(invalid_profile)?,
    )
    .map(Some)
    .map_err(|_| invalid_profile())
}

#[cfg(all(windows, feature = "acp-fixture"))]
fn parse_sha256(value: &OsStr) -> io::Result<[u8; 32]> {
    let value = value.to_str().ok_or_else(invalid_profile)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(invalid_profile());
    }
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| invalid_profile())?;
    }
    Ok(digest)
}

fn invalid_profile() -> io::Error {
    io::Error::new(ErrorKind::InvalidInput, "disposable profile is invalid")
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

#[cfg(windows)]
fn is_local_absolute(path: &Path) -> bool {
    use std::path::{Component, Prefix};

    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
    ) && path.is_absolute()
}

#[cfg(not(windows))]
fn is_local_absolute(path: &Path) -> bool {
    path.is_absolute()
}

pub(crate) fn normalize_without_creating(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid_profile());
    }
    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.try_exists()? {
        let name = existing.file_name().ok_or_else(invalid_profile)?;
        missing.push(name.to_owned());
        existing = existing.parent().ok_or_else(invalid_profile)?;
    }
    let mut normalized = existing.canonicalize()?;
    for name in missing.into_iter().rev() {
        normalized.push(name);
    }
    Ok(normalized)
}

fn require_marker(root: &Path) -> io::Result<()> {
    let marker = root.join(PROFILE_MARKER_NAME);
    let file = open_marker(&marker)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || is_reparse(&metadata)
        || metadata.len() != PROFILE_MARKER_BYTES.len() as u64
    {
        return Err(invalid_profile());
    }
    let mut bytes = Vec::with_capacity(PROFILE_MARKER_BYTES.len() + 1);
    file.take(PROFILE_MARKER_BYTES.len() as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes != PROFILE_MARKER_BYTES {
        return Err(invalid_profile());
    }
    Ok(())
}

#[cfg(windows)]
fn open_marker(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(windows))]
fn open_marker(path: &Path) -> io::Result<File> {
    File::open(path)
}

fn require_profile_inventory(root: &Path) -> io::Result<()> {
    let allowed = [PROFILE_MARKER_NAME, "durable", "webview", "browser"];
    let mut count = 0;
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        count += 1;
        if count > allowed.len()
            || !allowed
                .iter()
                .any(|name| entry.file_name().as_os_str() == OsStr::new(name))
        {
            return Err(invalid_profile());
        }
    }
    Ok(())
}

fn ensure_plain_child(root: &Path, name: &str) -> io::Result<PathBuf> {
    let path = root.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    require_plain_directory(&path)?;
    let canonical = path.canonicalize()?;
    if canonical.parent() != Some(root) {
        return Err(invalid_profile());
    }
    Ok(canonical)
}

pub(crate) fn require_plain_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir() || is_reparse(&metadata) {
        return Err(invalid_profile());
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        path::Path,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[cfg(all(windows, feature = "acp-fixture"))]
    use crate::acp::{intent::BindingId, service::CHARACTERIZATION_BINDING_ID};

    static NEXT_ROOT: AtomicUsize = AtomicUsize::new(1);

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sage-launch-profile-{label}-{}-{}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn marked(root: &Path) {
        fs::create_dir_all(root).unwrap();
        fs::File::create(root.join(PROFILE_MARKER_NAME))
            .unwrap()
            .write_all(PROFILE_MARKER_BYTES)
            .unwrap();
    }

    #[test]
    fn absent_selector_preserves_normal_paths_without_creating_them() {
        let normal_durable = root("normal-durable");
        let normal_webview = root("normal-webview");
        let profile =
            resolve_launch_profile(None, None, normal_durable.clone(), normal_webview.clone())
                .unwrap();

        assert_eq!(profile.durable_dir(), normal_durable);
        assert_eq!(profile.webview_dir(), None);
        assert_eq!(profile.project_root(), None);
        assert!(!normal_durable.exists());
        assert!(!normal_webview.exists());
    }

    #[test]
    fn marked_disjoint_root_resolves_only_fixed_profile_children() {
        let profile_root = root("disposable");
        let project_root = root("project");
        let normal_durable = root("normal-durable");
        let normal_webview = root("normal-webview");
        marked(&profile_root);
        fs::create_dir_all(&project_root).unwrap();

        let result = resolve_launch_profile(
            Some(profile_root.as_os_str()),
            Some(project_root.as_os_str()),
            normal_durable,
            normal_webview,
        );

        #[cfg(target_os = "macos")]
        {
            let error = result.expect_err("macOS disposable profiles are unsupported");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
            assert!(!profile_root.join("durable").exists());
            assert!(!profile_root.join("webview").exists());
        }

        #[cfg(not(target_os = "macos"))]
        {
            let profile = result.unwrap();
            let canonical = profile_root.canonicalize().unwrap();
            assert_eq!(profile.durable_dir(), canonical.join("durable"));
            assert_eq!(
                profile.webview_dir(),
                Some(canonical.join("webview").as_path())
            );
            assert_eq!(
                profile.project_root(),
                Some(project_root.canonicalize().unwrap().as_path())
            );
            assert!(profile.durable_dir().is_dir());
            assert!(profile.webview_dir().unwrap().is_dir());
        }
        fs::remove_dir_all(profile_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    #[test]
    fn characterization_selector_requires_complete_identity_and_disposable_project() {
        use sha2::{Digest, Sha256};

        let profile_root = root("characterization-profile");
        let project_root = root("characterization-project");
        let artifact_root = root("characterization-artifact");
        marked(&profile_root);
        fs::create_dir_all(&project_root).unwrap();
        fs::create_dir_all(&artifact_root).unwrap();
        let artifact = artifact_root.join("fixture.exe");
        fs::write(&artifact, b"bound fixture").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"bound fixture"));
        let bytes = fs::metadata(&artifact).unwrap().len().to_string();
        let profile = resolve_launch_profile(
            Some(profile_root.as_os_str()),
            Some(project_root.as_os_str()),
            root("normal-durable"),
            root("normal-webview"),
        )
        .unwrap();

        assert!(
            resolve_characterization_binding(&profile, None, None, None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            resolve_characterization_binding(
                &profile,
                Some(artifact.as_os_str()),
                Some(OsStr::new(&digest)),
                None,
                Some(OsStr::new("turn-success")),
            )
            .is_err()
        );
        let binding = resolve_characterization_binding(
            &profile,
            Some(artifact.as_os_str()),
            Some(OsStr::new(&digest)),
            Some(OsStr::new(&bytes)),
            Some(OsStr::new("turn-success")),
        )
        .unwrap()
        .unwrap();
        assert!(
            binding.accepts(&BindingId::new("bindingId", CHARACTERIZATION_BINDING_ID).unwrap())
        );

        fs::remove_dir_all(profile_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
        fs::remove_dir_all(artifact_root).unwrap();
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    #[test]
    fn codex_fixture_selector_requires_a_marked_proof_profile_and_complete_identity() {
        use sha2::{Digest, Sha256};

        let proof_root = root("cp14-proof");
        let profile_root = proof_root.join("profile");
        let project_root = root("cp14-project");
        let artifact_root = root("cp14-artifact");
        marked(&profile_root);
        fs::create_dir_all(&project_root).unwrap();
        fs::create_dir_all(&artifact_root).unwrap();
        let artifact = artifact_root.join("acp-adversarial-fixture.exe");
        fs::write(&artifact, b"cp14 fixture").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"cp14 fixture"));
        let bytes = fs::metadata(&artifact).unwrap().len().to_string();
        let profile = resolve_launch_profile(
            Some(profile_root.as_os_str()),
            Some(project_root.as_os_str()),
            root("cp14-normal-durable"),
            root("cp14-normal-webview"),
        )
        .unwrap();

        let selection = resolve_codex_fixture_selection(
            &profile,
            Some(artifact.as_os_str()),
            Some(OsStr::new(&digest)),
            Some(OsStr::new(&bytes)),
            Some(OsStr::new("codex-steer-lost-result")),
            &proof_root,
        )
        .unwrap()
        .expect("complete selector");
        selection.revalidate().unwrap();
        assert!(matches!(
            resolve_desktop_fixture_mode(&profile, [None; 4], [None; 4], &proof_root).unwrap(),
            DesktopFixtureMode::Ordinary
        ));
        assert!(
            resolve_desktop_fixture_mode(
                &profile,
                [
                    Some(artifact.as_os_str()),
                    Some(OsStr::new(&digest)),
                    Some(OsStr::new(&bytes)),
                    Some(OsStr::new("turn-success")),
                ],
                [
                    Some(artifact.as_os_str()),
                    Some(OsStr::new(&digest)),
                    Some(OsStr::new(&bytes)),
                    Some(OsStr::new("codex-steer-lost-result")),
                ],
                &proof_root,
            )
            .is_err()
        );
        let reconstructed = resolve_codex_fixture_selection(
            &profile,
            Some(artifact.as_os_str()),
            Some(OsStr::new(&digest)),
            Some(OsStr::new(&bytes)),
            Some(OsStr::new("codex-steer-lost-result")),
            &proof_root,
        )
        .unwrap()
        .unwrap();
        let first = crate::acp::bindings::codex::CodexBindingRegistration::desktop_fixture(
            selection.clone(),
        )
        .unwrap();
        let second = crate::acp::bindings::codex::CodexBindingRegistration::desktop_fixture(
            reconstructed.clone(),
        )
        .unwrap();
        let admitted = first.refresh().unwrap();
        assert!(admitted.is_desktop_fixture());
        assert!(admitted.capability_ceiling().unwrap().active_turn_steer);
        assert!(!admitted.model_discovery_available().unwrap());
        assert_eq!(
            admitted.binding_digest(),
            second.refresh().unwrap().binding_digest()
        );
        assert_ne!(
            admitted.binding_digest(),
            [
                0xf8, 0xc2, 0xb9, 0x95, 0xbe, 0x26, 0x3f, 0xe4, 0xbf, 0x00, 0x97, 0x40, 0x43, 0x31,
                0x26, 0x73, 0x37, 0x80, 0x81, 0x73, 0x4c, 0xa8, 0xc9, 0x81, 0x55, 0xd2, 0xa3, 0x38,
                0x87, 0x44, 0x4b, 0x9e
            ]
        );
        let environment = admitted.launch_environment();
        assert_eq!(environment.len(), 5);
        assert!(environment.iter().all(|(key, _)| matches!(
            key.as_str(),
            "USERPROFILE" | "APPDATA" | "LOCALAPPDATA" | "TEMP" | "TMP"
        )));
        for (key, value) in environment {
            assert!(
                !value.starts_with(r"\\?\"),
                "child environment must use a Win32 path"
            );
            let expected = if key == "USERPROFILE" {
                &selection.profile_root
            } else {
                &selection.durable_dir
            };
            assert_eq!(Path::new(value).canonicalize().unwrap(), *expected);
        }
        let registry = crate::acp::bindings::registry::BindingRegistry::desktop_codex_fixture(
            selection.clone(),
        )
        .unwrap();
        assert!(matches!(
            registry.codex().auth_provider,
            crate::acp::bindings::registry::AuthProvider::None
        ));
        assert!(
            !registry
                .codex()
                .admit()
                .unwrap()
                .has_model_discovery()
                .unwrap()
        );
        let catalog =
            crate::harnesses::HarnessCatalog::with_registry(std::sync::Arc::new(registry));
        catalog.apply_presence(Vec::new());
        let codex_row = catalog
            .definitions()
            .into_iter()
            .find(|row| row.id == "codex")
            .unwrap();
        assert_eq!(codex_row.install_state, "available");
        assert!(codex_row.sign_in.is_none());
        let receipt = selection.durable_dir.join("codex-fixture-launch-once-v1");
        assert!(
            !receipt.exists(),
            "catalog work must not consume the fixture"
        );
        let root_authority = || {
            crate::acp::bindings::codex::CodexProjectRootAuthority::for_test(
                crate::project_registry::ProjectRootAuthority::for_test(
                    crate::acp::contract::SageProjectId::parse("project-cp14").unwrap(),
                    crate::acp::contract::ProjectGeneration::ZERO,
                    crate::acp::contract::SageRootId::parse("root-cp14").unwrap(),
                    &project_root,
                )
                .unwrap(),
            )
        };
        for session_kind in ["discovery", "auth"] {
            let refused = crate::acp::bindings::codex::CodexProcessTransport::launch(
                std::sync::Arc::clone(&admitted),
                root_authority(),
                crate::acp::contract::RuntimeEpoch::new(1).unwrap(),
                crate::acp::bindings::codex::ProcessLaunchPolicy {
                    session_kind,
                    spawn_timeout: std::time::Duration::from_secs(1),
                    cleanup_end: std::time::Instant::now() + std::time::Duration::from_secs(2),
                    reader_slots: 1,
                },
                |_| Ok(()),
            );
            assert!(
                refused.is_err(),
                "fixture {session_kind} transport must refuse before spawn"
            );
            assert!(
                !receipt.exists(),
                "direct {session_kind} must not consume the fixture"
            );
        }
        let browser_mcp = crate::acp::mcp::McpAdmission::browser_stdio(
            selection.artifact_path.to_string_lossy().into_owned(),
            [
                ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
                ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
                ("SAGE_BROWSER_MCP_SESSION".into(), "session-cp14-mcp".into()),
                ("SAGE_BROWSER_MCP_TOKEN".into(), "secret-token".into()),
            ],
        )
        .unwrap();
        let mut mcp_runtime =
            crate::acp::bindings::codex::CodexRuntime::from_admitted_binding_with_mcp(
                std::sync::Arc::clone(&admitted),
                root_authority(),
                crate::acp::contract::RuntimeEpoch::new(1).unwrap(),
                crate::acp::contract::SageAgentSessionId::parse("session-cp14-mcp").unwrap(),
                browser_mcp,
            )
            .unwrap();
        assert!(mcp_runtime.launch_spec().is_err());
        assert!(
            !receipt.exists(),
            "MCP admission must not consume fixture case"
        );
        let foreign_root = root("cp14-foreign-project");
        fs::create_dir_all(&foreign_root).unwrap();
        let foreign_authority = crate::acp::bindings::codex::CodexProjectRootAuthority::for_test(
            crate::project_registry::ProjectRootAuthority::for_test(
                crate::acp::contract::SageProjectId::parse("project-cp14-foreign").unwrap(),
                crate::acp::contract::ProjectGeneration::ZERO,
                crate::acp::contract::SageRootId::parse("root-cp14-foreign").unwrap(),
                &foreign_root,
            )
            .unwrap(),
        );
        let mut foreign_runtime = crate::acp::bindings::codex::CodexRuntime::from_admitted_binding(
            std::sync::Arc::clone(&admitted),
            foreign_authority,
            crate::acp::contract::RuntimeEpoch::new(1).unwrap(),
            crate::acp::contract::SageAgentSessionId::parse("session-cp14-foreign").unwrap(),
        )
        .unwrap();
        assert!(foreign_runtime.launch_spec().is_err());
        assert!(
            !receipt.exists(),
            "foreign Project root must not consume the case"
        );
        let mut runtime = crate::acp::bindings::codex::CodexRuntime::from_admitted_binding(
            std::sync::Arc::clone(&admitted),
            root_authority(),
            crate::acp::contract::RuntimeEpoch::new(2).unwrap(),
            crate::acp::contract::SageAgentSessionId::parse("session-cp14").unwrap(),
        )
        .unwrap();
        let spec = runtime.launch_spec().unwrap();
        assert_eq!(
            spec.artifact.canonical_absolute_path,
            selection.artifact_path
        );
        assert_eq!(spec.artifact.sha256, selection.artifact_sha256);
        assert_eq!(spec.artifact.byte_len, selection.artifact_bytes);
        assert_eq!(
            spec.fixed_args
                .as_slice()
                .iter()
                .map(|arg| arg.to_str().unwrap())
                .collect::<Vec<_>>(),
            ["--case", "codex-steer-lost-result", "--seed", "0"]
        );
        assert_eq!(spec.environment.as_slice().len(), 5);
        assert!(spec.environment.as_slice().iter().all(|(key, _)| matches!(
            key.to_str().unwrap(),
            "USERPROFILE" | "APPDATA" | "LOCALAPPDATA" | "TEMP" | "TMP"
        )));
        assert!(receipt.is_file());
        assert_eq!(
            fs::read(&receipt).unwrap(),
            b"sage:cp14:codex-fixture-launch-once:v1\n"
        );
        assert!(reconstructed.consume_one_shot().is_err());
        let mut reopened_runtime =
            crate::acp::bindings::codex::CodexRuntime::from_admitted_binding(
                second.refresh().unwrap(),
                root_authority(),
                crate::acp::contract::RuntimeEpoch::new(3).unwrap(),
                crate::acp::contract::SageAgentSessionId::parse("session-cp14-reopened").unwrap(),
            )
            .unwrap();
        assert!(reopened_runtime.launch_spec().is_err());
        assert!(
            resolve_codex_fixture_selection(
                &profile,
                Some(artifact.as_os_str()),
                Some(OsStr::new(&digest)),
                None,
                Some(OsStr::new("codex-steer-lost-result")),
                &proof_root,
            )
            .is_err()
        );
        let linked_artifact = artifact_root.join("linked-fixture.exe");
        if std::os::windows::fs::symlink_file(&artifact, &linked_artifact).is_ok() {
            assert!(
                resolve_codex_fixture_selection(
                    &profile,
                    Some(linked_artifact.as_os_str()),
                    Some(OsStr::new(&digest)),
                    Some(OsStr::new(&bytes)),
                    Some(OsStr::new("codex-steer-lost-result")),
                    &proof_root,
                )
                .is_err(),
                "reparse artifact must be rejected"
            );
        }
        assert!(
            resolve_codex_fixture_selection(
                &profile,
                Some(artifact.as_os_str()),
                Some(OsStr::new(&digest)),
                Some(OsStr::new(&bytes)),
                Some(OsStr::new("unlisted-case")),
                &proof_root,
            )
            .is_err()
        );
        assert!(
            resolve_codex_fixture_selection(
                &profile,
                Some(artifact.as_os_str()),
                Some(OsStr::new(&digest)),
                Some(OsStr::new(&bytes)),
                Some(OsStr::new("codex-steer-lost-result")),
                &project_root,
            )
            .is_err()
        );
        fs::write(profile_root.join(PROFILE_MARKER_NAME), b"changed marker").unwrap();
        assert!(selection.revalidate().is_err());
        fs::write(profile_root.join(PROFILE_MARKER_NAME), PROFILE_MARKER_BYTES).unwrap();
        fs::write(&artifact, b"drifted").unwrap();
        assert!(selection.revalidate().is_err());
        first.invalidate_drift();
        assert!(
            first
                .authenticate_observed_for_test(artifact.clone(), "codex 0.146.0".into())
                .is_err()
        );
        assert!(
            resolve_codex_fixture_selection(
                &LaunchProfile::Normal {
                    durable_dir: root("cp14-ordinary")
                },
                Some(artifact.as_os_str()),
                Some(OsStr::new(&digest)),
                Some(OsStr::new(&bytes)),
                Some(OsStr::new("codex-steer-lost-result")),
                &proof_root,
            )
            .is_err()
        );

        fs::remove_dir_all(proof_root).unwrap();
        fs::remove_dir_all(project_root).unwrap();
        fs::remove_dir_all(artifact_root).unwrap();
        fs::remove_dir_all(foreign_root).unwrap();
    }

    #[test]
    fn selector_rejects_relative_missing_unmarked_and_overlapping_roots() {
        let normal = root("normal");
        marked(&normal);
        let nested = normal.join("nested");
        marked(&nested);
        let missing = root("missing");
        let unmarked = root("unmarked");
        fs::create_dir_all(&unmarked).unwrap();

        for configured in [
            PathBuf::from("relative-profile"),
            missing,
            unmarked.clone(),
            nested,
        ] {
            assert!(
                resolve_launch_profile(
                    Some(configured.as_os_str()),
                    None,
                    normal.clone(),
                    root("normal-webview"),
                )
                .is_err(),
                "accepted {}",
                configured.display()
            );
        }

        fs::remove_dir_all(normal).unwrap();
        fs::remove_dir_all(unmarked).unwrap();
    }

    #[test]
    fn project_selector_requires_a_disposable_disjoint_profile() {
        let project = root("project");
        fs::create_dir_all(&project).unwrap();
        assert!(
            resolve_launch_profile(
                None,
                Some(project.as_os_str()),
                root("normal-durable"),
                root("normal-webview"),
            )
            .is_err()
        );

        let profile = root("profile");
        marked(&profile);
        let nested_project = profile.join("project");
        fs::create_dir_all(&nested_project).unwrap();
        assert!(
            resolve_launch_profile(
                Some(profile.as_os_str()),
                Some(nested_project.as_os_str()),
                root("normal-durable"),
                root("normal-webview"),
            )
            .is_err()
        );
        assert!(!profile.join("durable").exists());
        assert!(!profile.join("webview").exists());
        fs::remove_dir_all(profile).unwrap();
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn selector_rejects_unknown_profile_content_before_creating_children() {
        let profile = root("profile-unknown");
        marked(&profile);
        fs::write(profile.join("unexpected"), b"content").unwrap();
        assert!(
            resolve_launch_profile(
                Some(profile.as_os_str()),
                None,
                root("normal-durable"),
                root("normal-webview"),
            )
            .is_err()
        );
        assert!(!profile.join("durable").exists());
        assert!(!profile.join("webview").exists());
        fs::remove_dir_all(profile).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn selector_rejects_symbolic_root_and_marker() {
        use std::os::unix::fs::symlink;

        let target = root("target");
        let linked_root = root("linked-root");
        marked(&target);
        symlink(&target, &linked_root).unwrap();
        assert!(
            resolve_launch_profile(
                Some(linked_root.as_os_str()),
                None,
                root("normal-durable"),
                root("normal-webview"),
            )
            .is_err()
        );
        fs::remove_file(linked_root).unwrap();

        fs::remove_file(target.join(PROFILE_MARKER_NAME)).unwrap();
        let marker_target = target.join("marker-target");
        fs::write(&marker_target, PROFILE_MARKER_BYTES).unwrap();
        symlink(&marker_target, target.join(PROFILE_MARKER_NAME)).unwrap();
        assert!(
            resolve_launch_profile(
                Some(target.as_os_str()),
                None,
                root("normal-durable"),
                root("normal-webview"),
            )
            .is_err()
        );
        fs::remove_dir_all(target).unwrap();
    }
}
