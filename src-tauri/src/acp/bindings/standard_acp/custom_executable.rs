//! On Windows, process creation silently runs `.cmd` and `.bat` files through
//! `cmd.exe /c`, which would interpret `&`, `%VAR%` and `^` in the
//! arguments, so only a native `.exe` is accepted there.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutableRefusal {
    NotNative,
    Missing,
    Link {
        target: Option<PathBuf>,
    },
    NotAFile,
    #[cfg(unix)]
    NotExecutable,
    Unusable,
}

impl ExecutableRefusal {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NotNative => "The executable must be a native .exe file. Scripts and shims such as .cmd, .bat or .com files run through a shell, so Sage does not launch them. An npm shim usually starts node with a script: choose the agent's own .exe, or install a native build of the agent.".into(),
            Self::Missing => "The executable does not exist.".into(),
            Self::Link { target: Some(target) } => format!(
                "The executable is a link. Sage launches only the file itself: use {} instead.",
                target.display()
            ),
            Self::Link { target: None } => {
                "The executable is a link whose target could not be read. Choose the file it points to.".into()
            }
            Self::NotAFile => "The executable is not a file.".into(),
            #[cfg(unix)]
            Self::NotExecutable => {
                "The file is not executable. Mark it executable (chmod +x) or choose the agent's binary.".into()
            }
            Self::Unusable => "The executable must sit in a folder on a local drive, on a path without links.".into(),
        }
    }
}

/// `windows` is a parameter so the rule is testable on every platform.
pub(crate) fn native_executable_name(path: &Path, windows: bool) -> bool {
    !windows
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

pub(crate) fn check_custom_executable(path: &Path) -> Result<std::fs::Metadata, ExecutableRefusal> {
    if !native_executable_name(path, cfg!(windows)) {
        return Err(ExecutableRefusal::NotNative);
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|_| ExecutableRefusal::Missing)?;
    if metadata.file_type().is_symlink() {
        return Err(ExecutableRefusal::Link {
            target: std::fs::canonicalize(path).ok(),
        });
    }
    if !metadata.is_file() {
        return Err(ExecutableRefusal::NotAFile);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(ExecutableRefusal::NotExecutable);
        }
    }
    if !crate::harness_discovery::authentic_regular_file(path) {
        return Err(ExecutableRefusal::Unusable);
    }
    Ok(metadata)
}

#[cfg(test)]
pub(crate) fn write_test_executable(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).expect("write executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("mark executable");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp(label: &str) -> TempRoot {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-custom-exe-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("temp root");
        TempRoot(root.canonicalize().expect("canonical root"))
    }

    #[test]
    fn windows_admits_only_a_native_exe_in_any_letter_case() {
        for admitted in [
            r"C:\agents\agent.exe",
            r"C:\agents\AGENT.EXE",
            r"C:\a\b.Exe",
        ] {
            assert!(
                native_executable_name(Path::new(admitted), true),
                "{admitted}"
            );
        }
        for refused in [
            r"C:\npm\agent.cmd",
            r"C:\npm\agent.CMD",
            r"C:\npm\agent.bat",
            r"C:\npm\agent.com",
            r"C:\npm\agent.ps1",
            r"C:\npm\agent",
            r"C:\npm\agent.exe.cmd",
        ] {
            assert!(
                !native_executable_name(Path::new(refused), true),
                "{refused}"
            );
        }
        assert!(native_executable_name(
            Path::new("/opt/agents/agent"),
            false
        ));
    }

    #[test]
    fn a_missing_path_or_a_directory_is_refused_with_its_reason() {
        let root = temp("shape");
        let name = if cfg!(windows) { "agent.exe" } else { "agent" };
        assert_eq!(
            check_custom_executable(&root.0.join(name)).err(),
            Some(ExecutableRefusal::Missing)
        );
        let folder = root.0.join(if cfg!(windows) {
            "folder.exe"
        } else {
            "folder"
        });
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            check_custom_executable(&folder).err(),
            Some(ExecutableRefusal::NotAFile)
        );
        write_test_executable(&root.0.join(name), b"agent");
        assert!(check_custom_executable(&root.0.join(name)).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_file_is_refused_before_it_could_reach_cmd() {
        let root = temp("batch");
        for name in ["agent.cmd", "agent.bat"] {
            let path = root.0.join(name);
            std::fs::write(&path, b"@echo off").unwrap();
            let refusal = check_custom_executable(&path).unwrap_err();
            assert_eq!(refusal, ExecutableRefusal::NotNative);
            assert!(refusal.message().contains(".exe"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_file_without_execute_permission_is_refused() {
        let root = temp("mode");
        let path = root.0.join("agent");
        std::fs::write(&path, b"agent").unwrap();
        assert_eq!(
            check_custom_executable(&path).err(),
            Some(ExecutableRefusal::NotExecutable)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_and_names_its_resolved_target() {
        let root = temp("link");
        let target = root.0.join("agent");
        write_test_executable(&target, b"agent");
        let link = root.0.join("agent-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let refusal = check_custom_executable(&link).unwrap_err();
        assert_eq!(
            refusal,
            ExecutableRefusal::Link {
                target: Some(target.clone())
            }
        );
        assert!(refusal.message().contains(&target.display().to_string()));
    }
}
