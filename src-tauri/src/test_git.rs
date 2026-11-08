use std::process::Command;

/// Git for test fixtures, isolated from the global and system config as Sage's
/// own Git calls are, so a developer's commit signing, hooks or aliases cannot
/// change what a fixture builds. Repository-local config still applies.
pub(crate) fn command() -> Command {
    let mut command = Command::new("git");
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    command
}
