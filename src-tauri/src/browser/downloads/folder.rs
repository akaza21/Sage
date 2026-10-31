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

/// Extensions that run code, install software or point at something that
/// does when opened from Explorer. Such a download always asks where to save.
const DANGEROUS_EXTENSIONS: &[&str] = &[
    "ade",
    "adp",
    "app",
    "appinstaller",
    "application",
    "appref-ms",
    "appx",
    "appxbundle",
    "asp",
    "bas",
    "bat",
    "cer",
    "chm",
    "cmd",
    "com",
    "cpl",
    "crt",
    "csh",
    "der",
    "desktopthemepack",
    "diagcab",
    "dll",
    "docm",
    "dotm",
    "exe",
    "fxp",
    "gadget",
    "grp",
    "hlp",
    "hta",
    "htt",
    "img",
    "inf",
    "ins",
    "iso",
    "isp",
    "its",
    "jar",
    "jnlp",
    "js",
    "jse",
    "ksh",
    "library-ms",
    "lnk",
    "mad",
    "maf",
    "mag",
    "mam",
    "maq",
    "mar",
    "mas",
    "mat",
    "mau",
    "mav",
    "maw",
    "mcf",
    "mda",
    "mdb",
    "mde",
    "mdt",
    "mdw",
    "mdz",
    "mht",
    "mhtml",
    "msc",
    "msh",
    "msh1",
    "msh1xml",
    "msh2",
    "msh2xml",
    "mshxml",
    "msi",
    "msix",
    "msixbundle",
    "msp",
    "mst",
    "msu",
    "ops",
    "osd",
    "pcd",
    "pif",
    "pl",
    "plg",
    "potm",
    "ppam",
    "ppsm",
    "pptm",
    "prf",
    "prg",
    "printerexport",
    "ps1",
    "ps1xml",
    "ps2",
    "ps2xml",
    "psc1",
    "psc2",
    "psd1",
    "psdm1",
    "psm1",
    "pssc",
    "py",
    "pyc",
    "pyo",
    "pyw",
    "pyz",
    "pyzw",
    "rdp",
    "reg",
    "scf",
    "scr",
    "sct",
    "search-ms",
    "settingcontent-ms",
    "sh",
    "shb",
    "shs",
    "sys",
    "theme",
    "themepack",
    "url",
    "vb",
    "vbe",
    "vbp",
    "vbs",
    "vhd",
    "vhdx",
    "vsmacros",
    "vsw",
    "webpnp",
    "website",
    "ws",
    "wsb",
    "wsc",
    "wsf",
    "wsh",
    "xbap",
    "xlam",
    "xll",
    "xlsm",
    "xltm",
    "xnk",
];

/// Windows device names that cannot be a file's stem.
const RESERVED_STEMS: &[&str] = &[
    "con",
    "conin$",
    "conout$",
    "prn",
    "aux",
    "nul",
    "com\u{b9}",
    "com\u{b2}",
    "com\u{b3}",
    "lpt\u{b9}",
    "lpt\u{b2}",
    "lpt\u{b3}",
    "com1",
    "com2",
    "com3",
    "com4",
    "com5",
    "com6",
    "com7",
    "com8",
    "com9",
    "lpt1",
    "lpt2",
    "lpt3",
    "lpt4",
    "lpt5",
    "lpt6",
    "lpt7",
    "lpt8",
    "lpt9",
];

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
/// download folder as Settings recorded it; `taken` reports destinations
/// already claimed by downloads still in flight.
pub(crate) fn plan(stored: Option<&Path>, suggested: &str, taken: impl Fn(&Path) -> bool) -> Plan {
    let file_name = sanitize_file_name(suggested);
    let Some(folder) = stored.and_then(admit_folder) else {
        return Plan::Ask {
            file_name,
            remember: true,
            directory: None,
        };
    };
    if is_dangerous(&file_name) {
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

/// Whether a file name runs code or installs software when opened.
pub(crate) fn is_dangerous(file_name: &str) -> bool {
    let name = file_name.trim_end_matches(['.', ' ']);
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        let extension = extension.to_ascii_lowercase();
        DANGEROUS_EXTENSIONS.contains(&extension.as_str())
    })
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

/// `folder/name`, or `folder/stem (n).ext` for the first free `n`, never a
/// path that already exists (a file, folder or link) or that another
/// download in flight has claimed. The result is always a direct child of
/// `folder`.
pub(crate) fn unique_destination(
    folder: &Path,
    file_name: &str,
    taken: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let (stem, extension) = split_extension(file_name);
    (0..=MAX_SUFFIX).find_map(|attempt| {
        let name = if attempt == 0 {
            file_name.to_owned()
        } else {
            format!("{stem} ({attempt}){extension}")
        };
        let candidate = folder.join(&name);
        let free = candidate.parent() == Some(folder)
            && candidate.file_name().and_then(|value| value.to_str()) == Some(name.as_str())
            && std::fs::symlink_metadata(&candidate).is_err()
            && !taken(&candidate);
        free.then_some(candidate)
    })
}

impl super::DownloadRegistry {
    /// Whether a download still in flight is saving to `path`, so a silent
    /// save picks another name instead of sharing its file.
    pub(crate) fn claims(&self, path: &std::path::Path) -> bool {
        // Case-insensitive, as on Windows and macOS file systems: a false
        // match only costs a ` (n)` suffix.
        let path = path.to_string_lossy().to_lowercase();
        self.destinations.iter().any(|(id, destination)| {
            destination.to_string_lossy().to_lowercase() == path
                && self
                    .entries
                    .get(id)
                    .is_some_and(|entry| super::is_active(entry.state))
        })
    }
}

#[cfg(test)]
#[path = "folder_tests.rs"]
mod tests;
