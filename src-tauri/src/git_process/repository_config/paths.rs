//! Reading the paths Git data files name, exactly as Git reads them, and
//! refusing any path Git and the filesystem calls here could resolve to
//! different entries.

use std::path::{Component, Path, PathBuf};

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

/// Resolves `target`, relative to `base` unless absolute, the way Git does,
/// for a path whose every directory the caller then proves is real (no link
/// or junction). Only `..` components that lead the path are accepted: they
/// climb out of `base`, which is real, so lexical and physical resolution
/// agree. Every other component must pass [`ambiguous_component`].
pub(super) fn resolve(base: &Path, target: &str) -> Option<PathBuf> {
    let (mut resolved, rest) = match absolute_prefix(target) {
        Some((prefix, rest)) => (prefix, rest),
        None => (base.to_path_buf(), target),
    };
    let parts = split(rest);
    let climbs = parts.iter().take_while(|part| **part == "..").count();
    if climbs > 0 && absolute_prefix(target).is_some() {
        return None;
    }
    for _ in 0..climbs {
        if !matches!(
            resolved.components().next_back(),
            Some(Component::Normal(_))
        ) {
            return None;
        }
        resolved.pop();
    }
    let names = &parts[climbs..];
    if (climbs == 0 && names.is_empty()) || names.iter().any(|part| ambiguous_component(part)) {
        return None;
    }
    resolved.extend(names);
    Some(resolved)
}

/// For an absolute `target`, the root it starts at, in the verbatim form
/// canonical paths use on Windows, and the rest of the path.
fn absolute_prefix(target: &str) -> Option<(PathBuf, &str)> {
    #[cfg(windows)]
    {
        let bytes = target.as_bytes();
        let drive = bytes.first().filter(|byte| byte.is_ascii_alphabetic())?;
        if bytes.get(1) != Some(&b':') || !matches!(bytes.get(2), Some(b'/' | b'\\')) {
            return None;
        }
        let root = PathBuf::from(format!(
            r"\\?\{}:\",
            char::from(*drive).to_ascii_uppercase()
        ));
        Some((root, &target[3..]))
    }
    #[cfg(not(windows))]
    {
        target
            .strip_prefix('/')
            .map(|rest| (PathBuf::from("/"), rest))
    }
}

/// True when `path` names a real location Git would read differently:
/// anything with a root or prefix that [`resolve`] does not build itself.
pub(super) fn has_root(path: &str) -> bool {
    Path::new(path).has_root() || path.contains(':')
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
