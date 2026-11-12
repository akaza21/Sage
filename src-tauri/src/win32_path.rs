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

/// A program and its arguments as a child process should receive them: each
/// verbatim path that has an equivalent ordinary spelling is rewritten, and
/// every other argument is kept as it is.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn ordinary_invocation(
    program: &Path,
    args: &[std::ffi::OsString],
) -> (PathBuf, Vec<std::ffi::OsString>) {
    (
        simplified(program),
        args.iter()
            .map(|arg| simplified(Path::new(arg)).into_os_string())
            .collect(),
    )
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
        .any(|part| part.ends_with('.') || part.ends_with(' ') || dos_device(part));
    (simple.len() < MAX_PATH && !rewritable).then_some(simple)
}

/// Whether Win32 reads `part` as a device rather than a file: `NUL`, `CON`,
/// `COM1` and the rest, with or without an extension.
fn dos_device(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).trim_end();
    let upper = stem.to_ascii_uppercase();
    let numbered = |prefix: &str| {
        stem.get(..3)
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
            && matches!(
                &stem[3..],
                "0" | "1"
                    | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
    };
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || numbered("COM")
        || numbered("LPT")
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
        for device in [
            "nul",
            "CON.js",
            "aux.tar.gz",
            "Com1",
            "lpt9.txt",
            "COM\u{b9}",
        ] {
            assert_eq!(
                simplified_text(&format!(r"\\?\C:\dir\{device}")),
                None,
                "{device}"
            );
        }
        assert_eq!(
            simplified_text(r"\\?\C:\console\nullable\com10").as_deref(),
            Some(r"C:\console\nullable\com10")
        );
        assert_eq!(simplified_text(r"\\?\Volume{x}\dir"), None);
        assert_eq!(simplified_text("/home/me"), None);
        assert_eq!(simplified_text(r"C:\already\plain"), None);
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplified_text(&long), None);
    }

    #[test]
    fn a_child_receives_ordinary_program_and_path_arguments() {
        let (program, args) = ordinary_invocation(
            Path::new(r"\\?\C:\nodejs\node.exe"),
            &[
                r"\\?\C:\npm\cli.js".into(),
                "--mode=rpc".into(),
                r"\\?\C:\odd.\keep".into(),
            ],
        );
        assert_eq!(program, PathBuf::from(r"C:\nodejs\node.exe"));
        assert_eq!(
            args,
            [r"C:\npm\cli.js", "--mode=rpc", r"\\?\C:\odd.\keep"].map(std::ffi::OsString::from)
        );
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
