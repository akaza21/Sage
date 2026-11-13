use std::ffi::OsString;
use std::path::PathBuf;

use super::super::supervisor::{BoundedEnvironment, BoundedOsArgs, LaunchError};
use super::artifact::VerifiedArtifact;

const NPM_CODEX_EXECUTION_LINE: &str = r#"endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\@openai\codex\bin\codex.js" %*"#;
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
    let script = codex_script_path(&artifact.canonical_path)?;
    if !script.is_file() {
        return Err(LaunchError::IdentityMismatch);
    }
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
    // The script comes from the verbatim canonical launcher path, which Node
    // cannot load as its main module; hand it the same file's ordinary name.
    args.push(crate::win32_path::simplified(&script).into_os_string());
    args.extend(fixed_args.as_slice().iter().cloned());
    Ok(BatchLaunchPlan { node, args })
}

fn observed_npm_codex_launcher(contents: &[u8]) -> bool {
    let Ok(contents) = std::str::from_utf8(contents) else {
        return false;
    };
    contents
        .lines()
        .any(|line| line.trim_end_matches('\r') == NPM_CODEX_EXECUTION_LINE)
        && contents
            .lines()
            .any(|line| line.trim_end_matches('\r') == "SETLOCAL")
        && contents
            .lines()
            .any(|line| line.trim_end_matches('\r') == "CALL :find_dp0")
}

fn codex_script_path(launcher: &std::path::Path) -> Result<PathBuf, LaunchError> {
    Ok(launcher
        .parent()
        .ok_or(LaunchError::InvalidAbsolutePath)?
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin")
        .join("codex.js"))
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
    use std::path::Path;

    use super::*;

    #[test]
    fn accepts_the_observed_npm_codex_launcher_and_resolves_node_from_path() {
        let launcher = Path::new(r"C:\Users\name\AppData\Roaming\npm\codex.cmd");
        let contents = concat!(
            "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n",
            ":start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n",
            "  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n",
            "  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\n",
            "endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  ",
            "\"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n"
        );
        assert!(observed_npm_codex_launcher(contents.as_bytes()));
        assert_eq!(
            codex_script_path(launcher).unwrap(),
            PathBuf::from(r"C:\Users\name\AppData\Roaming\npm")
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("bin")
                .join("codex.js")
        );
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
