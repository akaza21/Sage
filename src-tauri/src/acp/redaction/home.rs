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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomePrefix {
    path: String,
    windows: bool,
}

impl HomePrefix {
    /// `None` for a relative path or a filesystem root, which would turn
    /// every absolute path into `~`.
    pub(crate) fn new(path: &str) -> Option<Self> {
        let bytes = path.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        let unc = path.starts_with("\\\\");
        let windows = drive || unc;
        if !windows && !path.starts_with('/') {
            return None;
        }
        let trimmed =
            path.trim_end_matches(|character| character == '/' || (windows && character == '\\'));
        let root = if drive { 2 } else { usize::from(!unc) };
        if trimmed.len() <= root || trimmed.contains('\0') {
            return None;
        }
        Some(Self {
            path: trimmed.to_owned(),
            windows,
        })
    }

    /// The home folder of the user Sage runs as, read once.
    pub(crate) fn current() -> Option<&'static Self> {
        static CURRENT: OnceLock<Option<HomePrefix>> = OnceLock::new();
        CURRENT
            .get_or_init(|| {
                std::env::home_dir()
                    .as_deref()
                    .and_then(Path::to_str)
                    .and_then(Self::new)
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
            let matched = candidate
                .then(|| self.match_len(&bytes[index..]))
                .flatten()
                .filter(|length| !self.runs_on(&bytes[index + length..]));
            let Some(length) = matched else {
                index += 1;
                continue;
            };
            let start = self
                .file_url_start(bytes, index)
                .unwrap_or(index)
                .max(copied);
            if !is_boundary(bytes, start) {
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

    /// Resolves a `~` path, as `shorten` writes it, back under the home folder.
    pub(crate) fn expand(&self, path: &str) -> Option<PathBuf> {
        let rest = path.strip_prefix(TILDE)?;
        if rest.is_empty() {
            return Some(PathBuf::from(&self.path));
        }
        let separator = rest
            .chars()
            .next()
            .filter(|character| *character == '/' || (self.windows && *character == '\\'))?;
        Some(Path::new(&self.path).join(rest.trim_start_matches(separator)))
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

    /// Whether the home folder's last name runs on, as in `alice2`, `alice.bak`
    /// or, where a backslash is no separator, an escaped `alice\ smith`.
    fn runs_on(&self, rest: &[u8]) -> bool {
        match rest {
            [b'\\', ..] => !self.windows,
            [byte, ..] if is_name_byte(*byte) => true,
            [b'.', byte, ..] => is_name_byte(*byte),
            _ => false,
        }
    }

    fn file_url_start(&self, bytes: &[u8], index: usize) -> Option<usize> {
        let scheme: &[u8] = if self.windows && !self.path.starts_with("\\\\") {
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
mod tests {
    use super::*;

    fn shorten(home: &str, text: &str) -> String {
        HomePrefix::new(home).unwrap().shorten(text).into_owned()
    }

    #[test]
    fn windows_home_is_shortened_case_and_separator_insensitively() {
        let home = r"C:\Users\alice";
        for (input, expected) in [
            (r"see C:\Users\alice\proj\x.rs now", r"see ~\proj\x.rs now"),
            (r"see c:\users\ALICE\proj", r"see ~\proj"),
            ("see C:/Users/alice/proj", "see ~/proj"),
            (r#"path = "C:\\Users\\alice\\proj""#, r#"path = "~\\proj""#),
            (r"(C:\Users\alice)", "(~)"),
            (r"home is C:\Users\alice.", "home is ~."),
            ("file:///C:/Users/alice/proj/x.rs", "~/proj/x.rs"),
        ] {
            assert_eq!(shorten(home, input), expected, "{input}");
        }
    }

    #[test]
    fn home_name_with_spaces_is_matched_exactly() {
        assert_eq!(
            shorten(
                r"C:\Users\Alice Smith\",
                r"in C:\Users\Alice Smith\a b\c.rs"
            ),
            r"in ~\a b\c.rs"
        );
        assert_eq!(
            shorten("/Users/Alice Smith", "open /Users/Alice Smith/a b/c"),
            "open ~/a b/c"
        );
        assert_eq!(
            shorten("/Users/Alice Smith", "open /Users/Alice Smithers/c"),
            "open /Users/Alice Smithers/c"
        );
    }

    #[test]
    fn posix_home_is_shortened_only_as_a_whole_rooted_prefix() {
        let home = "/home/alice";
        for (input, expected) in [
            ("cd /home/alice/src && ls", "cd ~/src && ls"),
            ("`/home/alice/src`", "`~/src`"),
            ("[x](/home/alice/a.rs:3)", "[x](~/a.rs:3)"),
            ("file:///home/alice/a.png", "~/a.png"),
            ("/home/alice2/src", "/home/alice2/src"),
            ("/home/alice.bak/src", "/home/alice.bak/src"),
            ("/mnt/home/alice/src", "/mnt/home/alice/src"),
            ("/HOME/alice/src", "/HOME/alice/src"),
            (r"/home/alice\x", r"/home/alice\x"),
            ("/home/bob/src /srv/data", "/home/bob/src /srv/data"),
            ("a /home/alice b /home/alice/c", "a ~ b ~/c"),
        ] {
            assert_eq!(shorten(home, input), expected, "{input}");
        }
    }

    #[test]
    fn roots_and_relative_paths_are_not_homes() {
        for home in ["", "/", "//", r"C:\", "C:/", "home/alice", "~"] {
            assert_eq!(HomePrefix::new(home), None, "{home}");
        }
    }

    #[test]
    fn shortened_paths_expand_back_under_the_home_folder() {
        let posix = HomePrefix::new("/home/alice/").unwrap();
        assert_eq!(
            posix.expand("~/proj/a.rs"),
            Some(PathBuf::from("/home/alice/proj/a.rs"))
        );
        assert_eq!(posix.expand("~"), Some(PathBuf::from("/home/alice")));
        assert_eq!(posix.expand(r"~\proj"), None);
        assert_eq!(posix.expand("~bob/proj"), None);
        assert_eq!(posix.expand("proj/a.rs"), None);
        let windows = HomePrefix::new(r"C:\Users\alice").unwrap();
        assert_eq!(
            windows.expand(r"~\proj\a.rs"),
            Some(Path::new(r"C:\Users\alice").join(r"proj\a.rs"))
        );
    }
}
