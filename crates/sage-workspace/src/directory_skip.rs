use std::ffi::OsStr;

const SKIP_DIRECTORY_NAMES: &[&str] = &[".git", "node_modules"];

pub fn skip_workspace_directory_name(name: &OsStr) -> bool {
    SKIP_DIRECTORY_NAMES
        .iter()
        .any(|skip| directory_names_match(name, skip))
}

/// Write admission folds case on every platform: macOS and Windows volumes are
/// usually case-insensitive, so `.GIT` can still name the repository metadata.
pub fn protected_workspace_name(name: &str) -> bool {
    SKIP_DIRECTORY_NAMES
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
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
