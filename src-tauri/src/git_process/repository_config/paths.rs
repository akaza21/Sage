//! Reading the paths Git data files name, exactly as Git reads them, and
//! refusing any path Git and the filesystem calls here could resolve to
//! different entries.

use std::path::{Path, PathBuf};

/// The path a `gitdir:` file names, read as Git's `read_gitfile` reads it:
/// one trailing `\n`, then one trailing `\r`, are dropped, the rest must start
/// with `gitdir: `, and everything after that is the path byte for byte.
pub(super) fn gitfile_path(bytes: &[u8]) -> Option<&str> {
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    std::str::from_utf8(line.strip_prefix(b"gitdir: ")?).ok()
}

/// The path a `commondir` file names, read as Git's `get_common_dir_noenv`
/// reads it: every trailing `\n` and `\r` is dropped, and the rest is the
/// path byte for byte.
pub(super) fn commondir_path(bytes: &[u8]) -> Option<&str> {
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(byte, b'\n' | b'\r'))
        .map_or(0, |last| last + 1);
    std::str::from_utf8(&bytes[..end]).ok()
}

/// Splits a path on the separators Git uses: `/`, and on Windows also `\`.
pub(super) fn split(path: &str) -> Vec<&str> {
    #[cfg(windows)]
    return path.split(['/', '\\']).collect();
    #[cfg(not(windows))]
    return path.split('/').collect();
}

/// The paths the operating system walks to reach `target`, as Git reaches
/// it: joined to `base` as written unless absolute, never normalised here,
/// one entry per component and the full path last. Git and the filesystem
/// calls here then resolve every `..`, symlink and junction the same way:
/// lexically through Win32 on Windows, physically elsewhere. Every name other
/// than `.` and `..` must pass [`ambiguous_component`].
pub(super) fn written_prefixes(base: &Path, target: &str) -> Option<Vec<PathBuf>> {
    let (mut walked, rest) = match absolute_root(target) {
        Some((root, rest)) => (root, rest),
        None => (os_path(base), target),
    };
    let mut prefixes = Vec::new();
    for part in split(rest) {
        if !matches!(part, "." | "..") && ambiguous_component(part) {
            return None;
        }
        walked.push(part);
        prefixes.push(walked.clone());
    }
    (!prefixes.is_empty()).then_some(prefixes)
}

/// For an absolute `target`, the root it starts at and the rest of the path.
fn absolute_root(target: &str) -> Option<(PathBuf, &str)> {
    #[cfg(windows)]
    {
        let bytes = target.as_bytes();
        let drive = bytes.first().filter(|byte| byte.is_ascii_alphabetic())?;
        if bytes.get(1) != Some(&b':') || !matches!(bytes.get(2), Some(b'/' | b'\\')) {
            return None;
        }
        Some((
            PathBuf::from(format!("{}:\\", char::from(*drive))),
            &target[3..],
        ))
    }
    #[cfg(not(windows))]
    {
        target
            .strip_prefix('/')
            .map(|rest| (PathBuf::from("/"), rest))
    }
}

/// `path` in the form the operating system normalises the way Git's paths
/// are normalised. A canonical Windows path is verbatim (`\\?\C:\...`), which
/// would take `..` literally, so its plain form is used instead.
pub(super) fn os_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}
/// True when Git and the filesystem calls here could resolve `part` to
/// different entries, or when it is not one plain name at all. Fingerprints
/// use verbatim (`\\?\`) paths on Windows, which skip every Win32
/// normalisation, so each name Win32 would rewrite is refused instead:
/// a trailing `.` or space, a reserved device name, an 8.3 short-name alias
/// (`~` and a digit) and an alternate data stream (`:`). Everywhere, a
/// backslash, edge whitespace, control characters (NUL included) and the
/// code points HFS+ ignores are refused, and so are empty, `.` and `..`.
pub(super) fn ambiguous_component(part: &str) -> bool {
    let ignorable = |c: char| {
        matches!(
            c,
            '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}'
        )
    };
    let short_name = part
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit());
    let win32_rewrites = part.ends_with('.') || short_name || reserved_device_name(part);
    matches!(part, "" | "." | "..")
        || part.trim() != part
        || (cfg!(windows) && win32_rewrites)
        || part
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':') || ignorable(c))
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9` (and their
/// superscript-digit forms), `CONIN$` and `CONOUT$`, with or without an
/// extension, in any case.
pub(super) fn reserved_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|digit| {
                (digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
                    || matches!(digit, "\u{b9}" | "\u{b2}" | "\u{b3}")
            })
        }),
    }
}
