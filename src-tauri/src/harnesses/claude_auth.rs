use std::{ffi::OsString, path::PathBuf};

use super::HarnessCatalog;

impl HarnessCatalog {
    pub(crate) fn claude_cli_path(&self) -> Option<PathBuf> {
        let discovered = self.discovered("claude-code")?;
        if !crate::harness_discovery::authentic_harness_file(
            &discovered.executable,
            discovered.executable.file_name()?.to_str()?,
        ) {
            return None;
        }
        #[cfg(windows)]
        {
            if discovered.executable.file_name()?.to_str()? == "claude.exe" {
                return Some(discovered.executable);
            }
            if discovered.executable.file_name()?.to_str()? != "claude.cmd" {
                return None;
            }
            let root = discovered.executable.parent()?;
            let relative = std::path::Path::new("node_modules")
                .join("@anthropic-ai")
                .join("claude-code")
                .join("bin")
                .join("claude.exe");
            if crate::harness_discovery::authentic_npm_global_descendant(root, &relative) {
                return Some(root.join(relative));
            }
            None
        }
        #[cfg(not(windows))]
        {
            None
        }
    }

    pub(crate) fn claude_auth_environment(&self) -> Option<Vec<(OsString, OsString)>> {
        self.registry
            .claude_admission()
            .admitted()?
            .launch_environment()
            .ok()
    }
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
