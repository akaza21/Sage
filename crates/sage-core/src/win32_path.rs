//! Win32 path spelling and reserved device names.
//!
//! `std::fs::canonicalize` returns verbatim paths on Windows. Sage keeps them
//! for identity checks, but programs that receive a path as an argument or
//! setting expect the ordinary form: Node.js, for one, cannot start a main
//! script named `\\?\C:\...` ("EISDIR: illegal operation on a directory,
//! lstat 'C:'"). The ordinary form is returned only where it names the same
//! file, so the conversion never changes which file a program opens. Paths with
//! reserved device-name components stay verbatim, as do ordinary spellings of
//! 260 or more UTF-16 code units.

use std::path::{Path, PathBuf};

/// Win32's ordinary path limit in UTF-16 code units, including the terminating
/// NUL; paths reaching this length stay verbatim without long-path support.
const MAX_PATH: usize = 260;

/// `path` in its ordinary Win32 spelling when that names the same file;
/// otherwise `path` unchanged. Reserved device-name components and ordinary
/// spellings of `MAX_PATH` or more UTF-16 code units stay verbatim.
pub fn simplified(path: &Path) -> PathBuf {
    match path.to_str().and_then(simplified_text) {
        Some(text) => PathBuf::from(text),
        None => path.to_path_buf(),
    }
}

/// A program and its arguments as a child process should receive them: each
/// verbatim path that has an equivalent ordinary spelling is rewritten, and
/// every other argument is kept as it is.
pub fn ordinary_invocation(
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
/// reserved device-name component, a component ending in `.` or a space, or a
/// path of `MAX_PATH` or more UTF-16 code units.
fn simplified_text(text: &str) -> Option<String> {
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
    let rewritable = simple.split('\\').any(|part| {
        part.ends_with('.') || part.ends_with(' ') || windows_reserved_device_name(part)
    });
    (simple.encode_utf16().count() < MAX_PATH && !rewritable).then_some(simple)
}

/// Windows reserves these device names in every directory: case-insensitively,
/// whatever follows the first dot, and after trimming trailing spaces from the
/// stem. Through a verbatim path they become literal files that Win32 tools and
/// Git cannot handle, so write admission refuses them on every platform.
pub fn windows_reserved_device_name(filename: &str) -> bool {
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
    fn a_reserved_name_component_keeps_the_verbatim_path() {
        let path = Path::new(r"\\?\C:\projects\NUL\notes.txt");
        assert_eq!(simplified(path), path.to_path_buf());
    }

    #[test]
    fn a_multibyte_path_below_max_path_is_simplified() {
        // The ordinary spelling is 103 UTF-16 code units but 303 UTF-8 bytes.
        let ordinary = format!(r"C:\{}", "界".repeat(100));
        let verbatim = format!(r"\\?\{ordinary}");
        assert_eq!(simplified(Path::new(&verbatim)), PathBuf::from(ordinary));
    }

    #[test]
    fn invocation_paths_obey_the_utf16_max_path_boundary() {
        // Each supplementary character occupies two UTF-16 code units.
        let ordinary = format!(r"C:\{}", "\u{1f680}".repeat(128));
        let verbatim = format!(r"\\?\{ordinary}");
        let at_limit = format!("{verbatim}a");
        assert_eq!(
            simplified_text(&verbatim).as_deref(),
            Some(ordinary.as_str())
        );
        assert_eq!(simplified_text(&at_limit), None);
        assert_eq!(simplified(Path::new(&at_limit)), PathBuf::from(&at_limit));
        let (program, args) = ordinary_invocation(
            Path::new(&verbatim),
            &[verbatim.clone().into(), at_limit.clone().into()],
        );
        assert_eq!(program, PathBuf::from(&ordinary));
        assert_eq!(args, [ordinary, at_limit].map(std::ffi::OsString::from));
    }

    #[test]
    fn invocation_keeps_reserved_names_and_trailing_spaces_verbatim() {
        let paths = [
            r"\\?\C:\NUL\node.exe",
            r"\\?\C:\CLOCK$\cli.js",
            r"\\?\UNC\server\share\CONIN$\cli.js",
            r"\\?\C:\trailing \cli.js",
        ];
        let (program, args) =
            ordinary_invocation(Path::new(paths[0]), &paths.map(std::ffi::OsString::from));
        assert_eq!(program, PathBuf::from(paths[0]));
        assert_eq!(args, paths.map(std::ffi::OsString::from));
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
