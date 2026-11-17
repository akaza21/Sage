//! Shortens the current user's home folder to `~` in display text.
//!
//! The prefix is the real home folder this process runs under, not a
//! `/home/<name>` or `C:\Users\<name>` pattern: harnesses run on this machine
//! as this user, `~` only means *this* user's home, and an exact prefix
//! handles home names with spaces without guessing where the name ends.

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const TILDE: &str = "~";

/// Punctuation that may close a path in prose without being part of its name.
const CLOSING_PUNCTUATION: &[u8] = b".,;:!?)]}>\"'`*";

/// Longest file name most filesystems allow, which bounds how far a folder
/// beside the home folder is read.
pub(crate) const NAME_MAX: usize = 255;
/// Most folders beside the home folder whose names run on past a space.
const SIBLINGS_MAX: usize = 64;
/// Most entries read beside the home folder, so a shared folder holding
/// many homes bounds the startup read.
const ENTRIES_MAX: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomePrefix {
    path: String,
    windows: bool,
    /// What follows the home folder's name in each folder beside it whose
    /// name runs on past a space, such as ` Smith` for `alice Smith`; `None`
    /// for the folders read at startup by [`load_folders_beside_home`].
    siblings: Option<Vec<String>>,
}

static BESIDE_HOME: OnceLock<Vec<String>> = OnceLock::new();

/// Reads the folders beside the current home folder once, so that redaction
/// never waits on a slow or network disk. Sage calls it at startup off the
/// async runtime; until it returns, no folder is known to run on past a space.
pub(crate) fn load_folders_beside_home() {
    let Some(home) = std::env::home_dir() else {
        return;
    };
    let Some(prefix) = home.to_str().and_then(HomePrefix::new) else {
        return;
    };
    let names = home
        .parent()
        .and_then(|parent| std::fs::read_dir(parent).ok())
        .into_iter()
        .flatten()
        .take(ENTRIES_MAX)
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok());
    let _ = BESIDE_HOME.set(prefix.sibling_rests(names));
}

impl HomePrefix {
    /// A drive-letter or POSIX home. A verbatim `\\?\C:\…` home is read in its
    /// ordinary form, the way prose writes it. `None` for a relative or UNC
    /// path, or a filesystem root, which would turn every absolute path into `~`.
    pub(crate) fn new(path: &str) -> Option<Self> {
        let ordinary = sage_core::win32_path::simplified(Path::new(path));
        let path = ordinary.to_str()?;
        let bytes = path.as_bytes();
        let windows = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        if !windows && !path.starts_with('/') {
            return None;
        }
        let trimmed =
            path.trim_end_matches(|character| character == '/' || (windows && character == '\\'));
        let root = if windows { 2 } else { 0 };
        if trimmed.len() <= root || trimmed.contains('\0') {
            return None;
        }
        let path = if windows {
            trimmed.replace('/', "\\")
        } else {
            trimmed.to_owned()
        };
        Some(Self {
            path,
            windows,
            siblings: Some(Vec::new()),
        })
    }

    /// This home with the folders beside it, named as they are on disk,
    /// whose names are the home folder's followed by a space.
    #[cfg(test)]
    pub(crate) fn with_siblings(
        mut self,
        names: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Self {
        self.siblings = Some(self.sibling_rests(names));
        self
    }

    fn sibling_rests(&self, names: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
        let own = self
            .path
            .rsplit(|character| character == '/' || (self.windows && character == '\\'))
            .next()
            .unwrap_or_default()
            .to_owned();
        names
            .into_iter()
            .filter_map(|name| {
                let name = name.as_ref();
                if name.len() > NAME_MAX {
                    return None;
                }
                let rest = name.get(own.len()..)?;
                (self.same_name(&name.as_bytes()[..own.len()], own.as_bytes())
                    && rest.starts_with([' ', '\t']))
                .then(|| rest.to_owned())
            })
            .take(SIBLINGS_MAX)
            .collect()
    }

    /// The home folder of the user Sage runs as, with the folders beside it
    /// that [`load_folders_beside_home`] read.
    pub(crate) fn current() -> Option<&'static Self> {
        static CURRENT: OnceLock<Option<HomePrefix>> = OnceLock::new();
        CURRENT
            .get_or_init(|| {
                let mut prefix = Self::new(std::env::home_dir()?.to_str()?)?;
                prefix.siblings = None;
                Some(prefix)
            })
            .as_ref()
    }

    /// Replaces each whole occurrence of the home folder with `~`, keeping the
    /// separator that follows it as written. A `file://` URL that names the
    /// home folder becomes the plain `~` path.
    pub(crate) fn shorten<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let bytes = text.as_bytes();
        let first = self.path.as_bytes()[0];
        let mut output = String::new();
        let mut copied = 0;
        let mut index = 0;
        while index < bytes.len() {
            let candidate = if self.windows {
                bytes[index].eq_ignore_ascii_case(&first)
            } else {
                bytes[index] == first
            };
            let Some(length) = candidate.then(|| self.match_len(&bytes[index..])).flatten() else {
                index += 1;
                continue;
            };
            let start = self
                .file_url_start(bytes, index)
                .unwrap_or(index)
                .max(copied);
            if !is_boundary(bytes, start) || self.runs_on(bytes, start, index + length) {
                index += 1;
                continue;
            }
            output.push_str(&text[copied..start]);
            output.push_str(TILDE);
            index += length;
            copied = index;
        }
        if copied == 0 {
            return Cow::Borrowed(text);
        }
        output.push_str(&text[copied..]);
        Cow::Owned(output)
    }

    /// Resolves a `~` path, as `shorten` writes it, back under the home folder,
    /// written with the home folder's own separator throughout.
    pub(crate) fn expand(&self, path: &str) -> Option<PathBuf> {
        let rest = path.strip_prefix(TILDE)?;
        let separator = if self.windows { '\\' } else { '/' };
        let is_separator = |character: char| character == '/' || character == separator;
        if !rest.is_empty() && !rest.starts_with(is_separator) {
            return None;
        }
        let mut expanded = self.path.clone();
        for name in rest.split(is_separator).filter(|name| !name.is_empty()) {
            expanded.push(separator);
            expanded.push_str(name);
        }
        Some(PathBuf::from(expanded))
    }

    fn match_len(&self, text: &[u8]) -> Option<usize> {
        let mut cursor = 0;
        for &expected in self.path.as_bytes() {
            let actual = *text.get(cursor)?;
            if self.windows && matches!(expected, b'\\' | b'/') {
                if !matches!(actual, b'\\' | b'/') {
                    return None;
                }
                cursor += 1;
                if actual == b'\\' && text.get(cursor) == Some(&b'\\') {
                    cursor += 1;
                }
            } else if (self.windows && actual.eq_ignore_ascii_case(&expected)) || actual == expected
            {
                cursor += 1;
            } else {
                return None;
            }
        }
        Some(cursor)
    }

    /// Whether the home folder's last name runs on into a sibling such as
    /// `alice2`, `alice.bak`, `alice+backup` or `alice smith` or, where a
    /// backslash is no separator, an escaped `alice\ smith`. The name ends at a
    /// separator, or where nothing but closing punctuation is left of the word
    /// or of the quote or code span the path opens.
    fn runs_on(&self, text: &[u8], start: usize, end: usize) -> bool {
        let rest = &text[end..];
        match rest {
            [] | [b'/', ..] => return false,
            [b'\\', ..] => return !self.windows,
            _ => {}
        }
        if let Some(span) = quoted_rest(text, start, rest) {
            return !span.iter().all(|byte| CLOSING_PUNCTUATION.contains(byte));
        }
        if matches!(rest[0], b' ' | b'\t') {
            return self.name_continues_after_space(rest);
        }
        !rest
            .iter()
            .take_while(|byte| !byte.is_ascii_whitespace())
            .all(|byte| CLOSING_PUNCTUATION.contains(byte))
    }

    /// Whether the text after a space spells the rest of a folder that
    /// exists beside the home folder, as ` Smith\project` does after
    /// `C:\Users\Alice` when `C:\Users\Alice Smith` exists.
    fn name_continues_after_space(&self, rest: &[u8]) -> bool {
        let siblings = self
            .siblings
            .as_deref()
            .unwrap_or_else(|| BESIDE_HOME.get().map_or(&[], Vec::as_slice));
        siblings.iter().any(|sibling| {
            rest.get(..sibling.len())
                .is_some_and(|name| self.same_name(name, sibling.as_bytes()))
                && rest.get(sibling.len()).is_none_or(|&byte| {
                    self.is_separator(byte)
                        || byte.is_ascii_whitespace()
                        || CLOSING_PUNCTUATION.contains(&byte)
                })
        })
    }

    fn same_name(&self, text: &[u8], name: &[u8]) -> bool {
        if self.windows {
            text.eq_ignore_ascii_case(name)
        } else {
            text == name
        }
    }

    fn is_separator(&self, byte: u8) -> bool {
        byte == b'/' || (self.windows && byte == b'\\')
    }

    fn file_url_start(&self, bytes: &[u8], index: usize) -> Option<usize> {
        let scheme: &[u8] = if self.windows {
            b"file:///"
        } else {
            b"file://"
        };
        let start = index.checked_sub(scheme.len())?;
        bytes[start..index]
            .eq_ignore_ascii_case(scheme)
            .then_some(start)
    }
}

/// The rest of a quote or code span the path opens, up to its closing
/// delimiter on the same line.
fn quoted_rest<'a>(text: &[u8], start: usize, rest: &'a [u8]) -> Option<&'a [u8]> {
    let opener = *text.get(start.checked_sub(1)?)?;
    if !matches!(opener, b'"' | b'\'' | b'`') {
        return None;
    }
    let close = rest
        .iter()
        .position(|&byte| byte == opener || byte == b'\n')?;
    (rest[close] == opener).then(|| &rest[..close])
}

fn is_name_byte(byte: u8) -> bool {
    !byte.is_ascii() || byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

/// Whether a path may start at `index` rather than continue a longer path or word.
fn is_boundary(bytes: &[u8], index: usize) -> bool {
    index.checked_sub(1).is_none_or(|previous| {
        let byte = bytes[previous];
        !is_name_byte(byte) && !matches!(byte, b'/' | b'\\' | b'.' | b'~' | b'$' | b'%')
    })
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
