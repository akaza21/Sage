//! Where a user download is saved. The first download asks through the native
//! Save dialog and its folder becomes the saved download folder; later
//! downloads save there without asking. The saved folder is re-admitted on
//! every download (canonical, an existing directory, writable), so a folder
//! that was moved, deleted or made read-only asks again. Programs, scripts and
//! shortcuts always ask, even with a saved folder.
//!
//! Only Rust picks the folder: Settings opens a native folder picker through a
//! main-webview command, and no command takes a path from the renderer, a
//! page or the Agent.

use std::path::{Path, PathBuf};

mod file_types;

use super::{
    io::Deadline,
    placeholder::{self, Placeholder},
};

use file_types::RESERVED_STEMS;
pub(crate) use file_types::{is_dangerous, is_dangerous_mime};

const MAX_NAME_BYTES: usize = 200;
const MAX_EXTENSION_BYTES: usize = 24;
// Bound file-system attempts on a destination lane. Planning asks after
// exhaustion; publication fails without replacing any existing file.
const MAX_SUFFIX: u32 = 99;
const FALLBACK_NAME: &str = "download";

/// What to do with a download that is about to start.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Save here without asking; the placeholder is already reserved.
    Silent(Placeholder),
    /// Show the Save dialog. `remember` saves the chosen folder as the
    /// download folder; `directory` opens the dialog there.
    Ask {
        file_name: String,
        remember: bool,
        directory: Option<PathBuf>,
    },
}

#[cfg(test)]
pub(crate) fn plan(
    stored: Option<&Path>,
    suggested: &str,
    mime: Option<&str>,
    taken: impl Fn(&Path) -> bool,
) -> Plan {
    plan_before(stored, suggested, mime, taken, super::io::test_deadline())
}

/// Decides between saving silently and asking. `stored` is the saved
/// download folder as Settings recorded it; `mime` is the server's content
/// type; `taken` reports destinations already claimed by downloads still in
/// flight. A `Silent` path has been reserved on disk (see
/// [`unique_destination_before`]). This does file-system work, so it runs off the
/// WebView2 UI thread.
pub(crate) fn plan_before(
    stored: Option<&Path>,
    suggested: &str,
    mime: Option<&str>,
    taken: impl Fn(&Path) -> bool,
    deadline: Deadline,
) -> Plan {
    let file_name = sanitize_file_name(suggested);
    let dangerous = is_dangerous(&file_name) || mime.is_some_and(is_dangerous_mime);
    let Some(folder) = stored.and_then(|path| admit_folder_before(path, deadline)) else {
        return Plan::Ask {
            file_name,
            remember: stored.is_none() || !dangerous,
            directory: None,
        };
    };
    if dangerous {
        return Plan::Ask {
            file_name,
            remember: false,
            directory: Some(folder),
        };
    }
    match unique_destination_before(&folder, &file_name, taken, deadline) {
        Some(reserved) => Plan::Silent(reserved),
        None => Plan::Ask {
            file_name,
            remember: false,
            directory: Some(folder),
        },
    }
}

/// The folder of a path the user picked in the Save dialog, admitted for
/// storage, or `None` when it cannot be a download folder.
#[cfg(test)]
pub(crate) fn folder_of_choice(chosen: &Path) -> Option<PathBuf> {
    admit_folder(chosen.parent()?)
}

/// Canonical form of an existing, writable directory, or `None`.
#[cfg(test)]
pub(crate) fn admit_folder(path: &Path) -> Option<PathBuf> {
    admit_folder_before(path, super::io::test_deadline())
}

/// Canonical form of an existing, writable directory with the caller's
/// original deadline, including queue time. Stop between filesystem calls.
pub(crate) fn admit_folder_before(path: &Path, deadline: Deadline) -> Option<PathBuf> {
    deadline.check().ok()?;
    if !path.is_absolute() {
        return None;
    }
    let canonical = simplify_verbatim(std::fs::canonicalize(path).ok()?);
    deadline.check().ok()?;
    if !canonical.is_dir() {
        return None;
    }
    deadline.check().ok()?;
    if !is_writable(&canonical) {
        return None;
    }
    deadline.check().ok()?;
    Some(canonical)
}

/// Windows creates a probe file and deletes it through its own handle, so
/// nothing is ever deleted by name.
#[cfg(windows)]
fn is_writable(folder: &Path) -> bool {
    // A random (version 4 UUID) name, created exclusively, so no other
    // program can predict it or have a file there.
    let probe = folder.join(format!(".sage-write-probe-{}", uuid::Uuid::new_v4()));
    let Ok(placeholder) = placeholder::create(&probe) else {
        return false;
    };
    placeholder.delete_if_empty();
    true
}

/// Unix cannot delete by handle, so it writes no probe at all and asks the
/// kernel instead: may this process (by its effective ids) create entries in
/// the folder? Nothing is created, so nothing is ever deleted.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_writable(folder: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(folder.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string that outlives the call.
    let result = unsafe {
        libc::faccessat(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::W_OK | libc::X_OK,
            libc::AT_EACCESS,
        )
    };
    result == 0
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn is_writable(folder: &Path) -> bool {
    std::fs::metadata(folder).is_ok_and(|metadata| !metadata.permissions().readonly())
}

/// `std::fs::canonicalize` returns a verbatim path on Windows. WebView2 and the
/// Settings row want the ordinary form, which is equivalent for any path that
/// fits in `MAX_PATH` and has no component Windows would rewrite.
fn simplify_verbatim(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(simplified_verbatim_text) {
        Some(text) => PathBuf::from(text),
        None => path,
    }
}

fn simplified_verbatim_text(text: &str) -> Option<String> {
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
    (simple.len() < 260 && !rewritable).then_some(simple)
}

/// A single path component that is safe to create in the download folder:
/// no separators, drive or stream colons, control, bidi or format
/// characters, Windows device names, or trailing dots and spaces; bounded,
/// keeping its extension.
pub(crate) fn sanitize_file_name(suggested: &str) -> String {
    let base = suggested.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut name: String = base
        .chars()
        .filter(|character| !character.is_control() && !is_format_character(*character))
        .map(|character| {
            if matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                character
            }
        })
        .collect();
    name = name
        .trim_start_matches(' ')
        .trim_end_matches(['.', ' '])
        .to_owned();
    if name.chars().all(|character| character == '.') {
        return FALLBACK_NAME.to_owned();
    }
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED_STEMS.contains(&stem.to_ascii_lowercase().as_str()) {
        name.insert(0, '_');
    }
    bound(name)
}

fn is_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{00ad}'
            | '\u{061c}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2069}'
            | '\u{feff}'
    )
}

fn bound(name: String) -> String {
    if name.len() <= MAX_NAME_BYTES {
        return name;
    }
    let (stem, extension) = split_extension(&name);
    let extension = if extension.len() <= MAX_EXTENSION_BYTES {
        extension
    } else {
        ""
    };
    let mut stem = stem.to_owned();
    let budget = MAX_NAME_BYTES - extension.len();
    while stem.len() > budget {
        stem.pop();
    }
    let stem = stem.trim_end_matches(['.', ' ']);
    if stem.is_empty() {
        return FALLBACK_NAME.to_owned();
    }
    format!("{stem}{extension}")
}

/// Splits `report.tar.gz` into `report.tar` and `.gz`; a leading dot is part
/// of the stem, so `.env` has no extension.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 => name.split_at(index),
        _ => (name, ""),
    }
}

/// Candidate names shared by planning and atomic publication. Never leave
/// the admitted folder, and keep each search bounded.
pub(super) fn destination_candidates<'a>(
    folder: &'a Path,
    file_name: &'a str,
) -> impl Iterator<Item = PathBuf> + 'a {
    let (stem, extension) = split_extension(file_name);
    (0..=MAX_SUFFIX).map_while(move |attempt| {
        let name = if attempt == 0 {
            file_name.to_owned()
        } else {
            format!("{stem} ({attempt}){extension}")
        };
        let candidate = folder.join(&name);
        (candidate.parent() == Some(folder)
            && candidate.file_name().and_then(|value| value.to_str()) == Some(name.as_str()))
        .then_some(candidate)
    })
}

#[cfg(test)]
pub(crate) fn unique_destination(
    folder: &Path,
    file_name: &str,
    taken: impl Fn(&Path) -> bool,
) -> Option<Placeholder> {
    unique_destination_before(folder, file_name, taken, super::io::test_deadline())
}

/// Reserves the first available candidate with create_new, using the original
/// planning deadline across all attempts. This is only a planning claim:
/// publication claims the final name again atomically without replacement.
fn unique_destination_before(
    folder: &Path,
    file_name: &str,
    taken: impl Fn(&Path) -> bool,
    deadline: Deadline,
) -> Option<Placeholder> {
    unique_destination_with(folder, file_name, taken, deadline, placeholder::create)
}

fn unique_destination_with(
    folder: &Path,
    file_name: &str,
    taken: impl Fn(&Path) -> bool,
    deadline: Deadline,
    mut create: impl FnMut(&Path) -> std::io::Result<Placeholder>,
) -> Option<Placeholder> {
    for candidate in destination_candidates(folder, file_name) {
        deadline.check().ok()?;
        if taken(&candidate) {
            continue;
        }
        match create(&candidate) {
            Ok(placeholder) => return Some(placeholder),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Destinations of downloads still in flight, compared without case as on
/// Windows and macOS file systems (a false match only costs a ` (n)`).
#[derive(Debug, Default)]
pub(crate) struct Claimed(Vec<String>);

impl Claimed {
    pub(crate) fn contains(&self, path: &Path) -> bool {
        let path = path.to_string_lossy().to_lowercase();
        self.0.contains(&path)
    }
}

impl super::DownloadRegistry {
    pub(crate) fn claimed(&self) -> Claimed {
        Claimed(
            self.destinations
                .iter()
                .filter(|(id, _)| {
                    self.entries
                        .get(id)
                        .is_some_and(|entry| super::is_active(entry.state))
                })
                .map(|(_, path)| path.to_string_lossy().to_lowercase())
                .collect(),
        )
    }
}

#[cfg(test)]
#[path = "folder_tests.rs"]
mod tests;
