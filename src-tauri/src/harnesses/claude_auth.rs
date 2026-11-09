use std::{ffi::OsString, path::PathBuf};

use super::HarnessCatalog;

impl HarnessCatalog {
    pub(crate) fn claude_cli_path(&self) -> Option<PathBuf> {
        let discovered = self.discovered("claude-code")?;
        if cfg!(windows) {
            native_claude_executable(&discovered.executable)
        } else {
            None
        }
    }

    pub(crate) fn claude_auth_environment(&self) -> Option<Vec<(OsString, OsString)>> {
        if self.registry.claude_sdk_admission().is_available() {
            return Some(crate::acp::bindings::claude::sdk::launch_environment());
        }
        self.registry
            .claude_admission()
            .admitted()?
            .launch_environment()
            .ok()
    }
}

/// The native `claude` executable behind a discovered launcher. On Windows an
/// npm install's `claude.cmd` shim stands for the package's own `claude.exe`,
/// which Sage runs directly.
pub(crate) fn native_claude_executable(executable: &std::path::Path) -> Option<PathBuf> {
    let name = executable.file_name()?.to_str()?;
    if !crate::harness_discovery::authentic_harness_file(executable, name) {
        return None;
    }
    if !cfg!(windows) || name == "claude.exe" {
        return Some(executable.to_owned());
    }
    if name != "claude.cmd" {
        return None;
    }
    let root = executable.parent()?;
    let relative = std::path::Path::new("node_modules")
        .join("@anthropic-ai")
        .join("claude-code")
        .join("bin")
        .join("claude.exe");
    crate::harness_discovery::authentic_npm_global_descendant(root, &relative)
        .then(|| root.join(relative))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::harness_discovery::InstalledHarness;

    #[test]
    fn auth_uses_only_a_valid_native_claude_executable() {
        let root = std::env::temp_dir().join(format!("sage-claude-cli-{}", uuid::Uuid::new_v4()));
        let native = root.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(&native, b"fixture").unwrap();
        let shim = root.join("claude.cmd");
        std::fs::write(&shim, b"fixture").unwrap();
        let catalog = HarnessCatalog::default();
        catalog.apply_presence(vec![InstalledHarness {
            id: "claude-code",
            executable: shim,
            version: None,
        }]);
        assert_eq!(catalog.claude_cli_path(), Some(native.clone()));
        std::fs::remove_file(&native).unwrap();
        assert_eq!(catalog.claude_cli_path(), None);
        std::fs::write(&native, b"fixture").unwrap();
        catalog.apply_presence(vec![InstalledHarness {
            id: "claude-code",
            executable: native.clone(),
            version: None,
        }]);
        assert_eq!(catalog.claude_cli_path(), Some(native));
        std::fs::remove_dir_all(root).unwrap();
    }
}
