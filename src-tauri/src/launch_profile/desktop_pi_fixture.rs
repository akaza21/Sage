use super::*;

pub(super) fn environment_values() -> [Option<std::ffi::OsString>; 4] {
    fixture_selector::PI_ENVIRONMENT_KEYS.map(std::env::var_os)
}

pub(super) fn resolve(
    profile: &LaunchProfile,
    values: [Option<&OsStr>; 4],
    proof_root: &Path,
) -> io::Result<Option<FixtureSelection>> {
    fixture_selector::resolve_selection(profile, values, proof_root, "pi")
}
