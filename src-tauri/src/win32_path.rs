//! The ordinary Win32 spelling of a verbatim (`\\?\`) path.
//!
//! `std::fs::canonicalize` returns verbatim paths on Windows. Sage keeps them
//! for identity checks, but programs that receive a path as an argument or
//! setting expect the ordinary form: Node.js, for one, cannot start a main
//! script named `\\?\C:\...` ("EISDIR: illegal operation on a directory,
//! lstat 'C:'"). The ordinary form is returned only where it names the same
//! file, so the conversion never changes which file a program opens.

use std::path::{Path, PathBuf};

/// Win32 rewrites a path longer than `MAX_PATH` only with long-path support,
/// so longer paths stay verbatim.
const MAX_PATH: usize = 260;

/// `path` in its ordinary Win32 spelling when that names the same file;
/// otherwise `path` unchanged.
pub(crate) fn simplified(path: &Path) -> PathBuf {
    match path.to_str().and_then(simplified_text) {
        Some(text) => PathBuf::from(text),
        None => path.to_path_buf(),
    }
}

/// The ordinary spelling of a verbatim local-drive or UNC path, or `None` when
/// `text` is not verbatim or Win32 would read its ordinary form differently: a
/// component ending in `.` or a space, or a path of `MAX_PATH` or more.
pub(crate) fn simplified_text(text: &str) -> Option<String> {
    let simple = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        let rest = text.strip_prefix(r"\\?\")?;
        let bytes = rest.as_bytes();
        if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
            return None;
        }
        rest.to_owned()
    };
    let rewritable = simple
        .split('\\')
        .any(|part| part.ends_with('.') || part.ends_with(' '));
    (simple.len() < MAX_PATH && !rewritable).then_some(simple)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_paths_are_simplified_only_when_equivalent() {
        assert_eq!(
            simplified_text(r"\\?\C:\downloads").as_deref(),
            Some(r"C:\downloads")
        );
        assert_eq!(
            simplified_text(r"\\?\UNC\server\share\dl").as_deref(),
            Some(r"\\server\share\dl")
        );
        assert_eq!(simplified_text(r"\\?\C:\odd.\dir"), None);
        assert_eq!(simplified_text(r"\\?\C:\trailing \dir"), None);
        assert_eq!(simplified_text(r"\\?\Volume{x}\dir"), None);
        assert_eq!(simplified_text("/home/me"), None);
        assert_eq!(simplified_text(r"C:\already\plain"), None);
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplified_text(&long), None);
    }

    #[test]
    fn a_path_that_cannot_be_simplified_is_kept_as_is() {
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplified(Path::new(&long)), PathBuf::from(&long));
        assert_eq!(
            simplified(Path::new(
                r"\\?\C:\npm\node_modules\@openai\codex\bin\codex.js"
            )),
            PathBuf::from(r"C:\npm\node_modules\@openai\codex\bin\codex.js")
        );
    }
}
