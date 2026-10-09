#[cfg(any(test, all(windows, feature = "acp-fixture")))]
use super::*;

#[cfg(all(windows, feature = "acp-fixture"))]
fn codex_fixture_environment_values() -> [Option<std::ffi::OsString>; 4] {
    [
        std::env::var_os("SAGE_CODEX_FIXTURE_PATH"),
        std::env::var_os("SAGE_CODEX_FIXTURE_SHA256"),
        std::env::var_os("SAGE_CODEX_FIXTURE_BYTES"),
        std::env::var_os("SAGE_CODEX_FIXTURE_CASE"),
    ]
}

#[cfg(all(test, not(all(windows, feature = "acp-fixture", debug_assertions))))]
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
    #[cfg(debug_assertions)]
    let claude = desktop_claude_fixture::environment_values();
    #[cfg(debug_assertions)]
    if claude.iter().any(Option::is_some) {
        let selection = desktop_claude_fixture::resolve(
            profile,
            claude.each_ref().map(Option::as_deref),
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("checkout parent")
                .join("work/cp-native-proof"),
        )?
        .ok_or_else(invalid_profile)?;
        return Ok(DesktopFixtureMode::ClaudeFixture(selection));
    }
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
    #[cfg(debug_assertions)]
    ClaudeFixture(FixtureSelection),
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
pub(crate) fn resolve_codex_fixture_selection(
    profile: &LaunchProfile,
    artifact_path: Option<&OsStr>,
    artifact_sha256: Option<&OsStr>,
    artifact_bytes: Option<&OsStr>,
    case: Option<&OsStr>,
    proof_root: &Path,
) -> io::Result<Option<CodexFixtureSelection>> {
    fixture_selector::resolve_selection(
        profile,
        [artifact_path, artifact_sha256, artifact_bytes, case],
        proof_root,
        "codex",
    )
}
