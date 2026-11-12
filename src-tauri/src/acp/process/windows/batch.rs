use std::ffi::OsString;
use std::path::PathBuf;

use super::super::supervisor::{BoundedEnvironment, BoundedOsArgs, LaunchError};
use super::artifact::VerifiedArtifact;

/// Every npm shim for the package names its folder; the rest of the shim
/// is npm's template, which changes between npm releases.
const CODEX_PACKAGE_DIRECTORY: &str = r"\node_modules\@openai\codex\";
const MAX_LAUNCHER_BYTES: u64 = 16 * 1024;

pub(super) struct BatchLaunchPlan {
    pub(super) node: PathBuf,
    pub(super) args: Vec<OsString>,
}

pub(super) fn parse_npm_codex_launcher(
    artifact: &VerifiedArtifact,
    environment: &BoundedEnvironment,
    fixed_args: &BoundedOsArgs,
) -> Result<BatchLaunchPlan, LaunchError> {
    let contents = artifact.read_launcher_bytes(MAX_LAUNCHER_BYTES)?;
    if !observed_npm_codex_launcher(&contents) {
        return Err(LaunchError::IdentityMismatch);
    }
    let launcher_directory = artifact
        .canonical_path
        .parent()
        .ok_or(LaunchError::InvalidAbsolutePath)?;
    // The module the installed release's `bin.codex` names, never a path
    // read from the shim, so an npm or Codex upgrade keeps launching.
    let script =
        crate::harness_discovery::npm_package_bin(launcher_directory, "@openai/codex", "codex")
            .ok_or(LaunchError::IdentityMismatch)?;
    let adjacent_node = launcher_directory.join("node.exe");
    let node = if adjacent_node.is_file() {
        adjacent_node
    } else {
        let path = environment
            .as_slice()
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
            .ok_or(LaunchError::IdentityMismatch)?;
        node_from_path(path)?
    };
    let mut args = Vec::with_capacity(fixed_args.as_slice().len().saturating_add(1));
    // Launch rewrites the verbatim script path to its ordinary spelling.
    args.push(script.into_os_string());
    args.extend(fixed_args.as_slice().iter().cloned());
    Ok(BatchLaunchPlan { node, args })
}

/// Whether the shim is npm's launcher for the Codex package, whatever
/// template the installing npm release used. Sage never runs the shim.
fn observed_npm_codex_launcher(contents: &[u8]) -> bool {
    std::str::from_utf8(contents).is_ok_and(|contents| {
        contents
            .to_ascii_lowercase()
            .contains(&CODEX_PACKAGE_DIRECTORY.to_ascii_lowercase())
    })
}

fn node_from_path(path: &std::ffi::OsStr) -> Result<PathBuf, LaunchError> {
    path.to_string_lossy()
        .split(';')
        .filter(|directory| !directory.is_empty())
        .map(PathBuf::from)
        .map(|directory| directory.join("node.exe"))
        .find(|node| node.is_file())
        .ok_or(LaunchError::IdentityMismatch)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn accepts_npm_codex_shims_from_any_npm_template() {
        let current = concat!(
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n",
            ":start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n",
            "  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n",
            "  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
            "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  ",
            "\"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n"
        );
        // A different template, as a later npm might write it.
        let rewritten =
            "@echo off\r\nnode \"%~dp0\\node_modules\\@OpenAI\\Codex\\bin\\codex.js\" %*\r\n";
        assert!(observed_npm_codex_launcher(current.as_bytes()));
        assert!(observed_npm_codex_launcher(rewritten.as_bytes()));
    }

    #[test]
    fn rejects_launcher_content_that_is_not_the_observed_npm_codex_shape() {
        assert!(!observed_npm_codex_launcher(
            b"@echo off\r\npowershell -EncodedCommand arbitrary\r\n"
        ));
    }

    #[test]
    fn resolves_only_an_existing_absolute_node_executable() {
        let directory = std::env::temp_dir().join("sage-batch-plan-node-test");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let node = directory.join("node.exe");
        fs::write(&node, b"node").unwrap();
        let path = OsString::from(directory.as_os_str());
        assert_eq!(node_from_path(&path).unwrap(), node);
        fs::remove_dir_all(directory).unwrap();
    }
}
