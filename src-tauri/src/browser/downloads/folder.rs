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

use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};

mod file_types;

use file_types::RESERVED_STEMS;
pub(crate) use file_types::{is_dangerous, is_dangerous_mime};

const MAX_NAME_BYTES: usize = 200;
const MAX_EXTENSION_BYTES: usize = 24;
// Each attempt is a file-system check on the WebView2 UI thread, so the
// search stays short; past it the download asks instead.
const MAX_SUFFIX: u32 = 99;
const FALLBACK_NAME: &str = "download";

/// What to do with a download that is about to start.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// Save here without asking.
    Silent(PathBuf),
    /// Show the Save dialog. `remember` saves the chosen folder as the
    /// download folder; `directory` opens the dialog there.
    Ask {
        file_name: String,
        remember: bool,
        directory: Option<PathBuf>,
    },
}

/// Decides between saving silently and asking. `stored` is the saved
/// download folder as Settings recorded it; `mime` is the server's content
/// type; `taken` reports destinations already claimed by downloads still in
/// flight. A `Silent` path has been reserved on disk (see
/// [`unique_destination`]). This does file-system work, so it runs off the
/// WebView2 UI thread.
pub(crate) fn plan(
    stored: Option<&Path>,
    suggested: &str,
    mime: Option<&str>,
    taken: impl Fn(&Path) -> bool,
) -> Plan {
    let file_name = sanitize_file_name(suggested);
    let Some(folder) = stored.and_then(admit_folder) else {
        return Plan::Ask {
            file_name,
            remember: true,
            directory: None,
        };
    };
    if is_dangerous(&file_name) || mime.is_some_and(is_dangerous_mime) {
        return Plan::Ask {
            file_name,
            remember: false,
            directory: Some(folder),
        };
    }
    match unique_destination(&folder, &file_name, taken) {
        Some(path) => Plan::Silent(path),
        None => Plan::Ask {
            file_name,
            remember: false,
            directory: Some(folder),
        },
    }
}

/// The folder of a path the user picked in the Save dialog, admitted for
/// storage, or `None` when it cannot be a download folder.
pub(crate) fn folder_of_choice(chosen: &Path) -> Option<PathBuf> {
    admit_folder(chosen.parent()?)
}

/// Canonical form of an existing, writable directory, or `None`.
pub(crate) fn admit_folder(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let canonical = simplify_verbatim(std::fs::canonicalize(path).ok()?);
    if !canonical.is_dir() || !is_writable(&canonical) {
        return None;
    }
    Some(canonical)
}

/// Creates and removes a uniquely named probe file. Directory permission
/// bits do not say whether this user can write there (ACLs, read-only
/// shares), so only a real create does.
fn is_writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".sage-write-probe-{}", uuid::Uuid::new_v4()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// `std::fs::canonicalize` returns `\\?\C:\…` on Windows. WebView2 and the
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

/// Reserves `folder/name`, or `folder/stem (n).ext` for the first free `n`,
/// by creating it empty with `create_new`, so no existing file, folder or
/// link is ever reused and two downloads (or another program) cannot claim
/// the same name. Any error other than "already exists" fails closed, and
/// the download asks instead. The result is always a direct child of
/// `folder`; WebView2 writes over the empty placeholder, and
/// [`release_reservation`] removes it if the download never does.
pub(crate) fn unique_destination(
    folder: &Path,
    file_name: &str,
    taken: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let (stem, extension) = split_extension(file_name);
    for attempt in 0..=MAX_SUFFIX {
        let name = if attempt == 0 {
            file_name.to_owned()
        } else {
            format!("{stem} ({attempt}){extension}")
        };
        let candidate = folder.join(&name);
        if candidate.parent() != Some(folder)
            || candidate.file_name().and_then(|value| value.to_str()) != Some(name.as_str())
        {
            return None;
        }
        if taken(&candidate) {
            continue;
        }
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Some(candidate),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Removes a placeholder [`unique_destination`] reserved, but only while it
/// is still an empty file: a finished or partly written download is kept.
pub(crate) fn release_reservation(path: &Path) {
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == 0) {
        let _ = std::fs::remove_file(path);
    }
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
