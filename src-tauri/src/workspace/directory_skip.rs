use std::ffi::OsStr;

const SKIP_DIRECTORY_NAMES: &[&str] = &[".git", "node_modules"];

pub(crate) fn skip_workspace_directory_name(name: &OsStr) -> bool {
    SKIP_DIRECTORY_NAMES
        .iter()
        .any(|skip| directory_names_match(name, skip))
}

/// Write admission folds case on every platform: macOS and Windows volumes are
/// usually case-insensitive, so `.GIT` can still name the repository metadata.
pub(crate) fn protected_workspace_name(name: &str) -> bool {
    SKIP_DIRECTORY_NAMES
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
}

/// Windows reserves these device names in every directory: case-insensitively,
/// whatever follows the first dot, and after trimming trailing spaces from the
/// stem. Through a verbatim path they become literal files that Win32 tools and
/// Git cannot handle, so write admission refuses them on every platform.
pub(crate) fn windows_reserved_device_name(filename: &str) -> bool {
    let stem = filename.split('.').next().unwrap_or(filename).trim_end();
    let upper = stem.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }
    let suffix = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"));
    suffix.is_some_and(|suffix| {
        matches!(
            suffix,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

fn directory_names_match(name: &OsStr, expected: &str) -> bool {
    if cfg!(windows) {
        name.to_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    } else {
        name == OsStr::new(expected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_git_and_node_modules() {
        assert!(skip_workspace_directory_name(OsStr::new(".git")));
        assert!(skip_workspace_directory_name(OsStr::new("node_modules")));
        assert!(!skip_workspace_directory_name(OsStr::new("src")));
        assert!(!skip_workspace_directory_name(OsStr::new("vendor")));
        assert!(!skip_workspace_directory_name(OsStr::new(
            "node_modules-guide"
        )));
    }

    #[cfg(windows)]
    #[test]
    fn windows_directory_names_are_case_insensitive() {
        assert!(skip_workspace_directory_name(OsStr::new(".GIT")));
        assert!(skip_workspace_directory_name(OsStr::new("Node_Modules")));
    }

    #[cfg(unix)]
    #[test]
    fn unix_directory_names_are_case_sensitive() {
        assert!(!skip_workspace_directory_name(OsStr::new(".GIT")));
        assert!(!skip_workspace_directory_name(OsStr::new("Node_Modules")));
    }
}
