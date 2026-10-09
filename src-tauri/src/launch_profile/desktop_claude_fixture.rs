use super::*;

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(super) fn environment_values() -> [Option<std::ffi::OsString>; 4] {
    [
        "SAGE_CLAUDE_FIXTURE_PATH",
        "SAGE_CLAUDE_FIXTURE_SHA256",
        "SAGE_CLAUDE_FIXTURE_BYTES",
        "SAGE_CLAUDE_FIXTURE_CASE",
    ]
    .map(std::env::var_os)
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
pub(super) fn resolve(
    profile: &LaunchProfile,
    values: [Option<&OsStr>; 4],
    proof_root: &Path,
) -> io::Result<Option<FixtureSelection>> {
    fixture_selector::resolve_selection(profile, values, proof_root, "claude")
}
