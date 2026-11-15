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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HomePrefix {
    path: String,
    windows: bool,
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
        Some(Self { path, windows })
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
    /// `alice2`, `alice.bak` or `alice+backup` or, where a backslash is no
    /// separator, an escaped `alice\ smith`. The name ends only at a separator
    /// or where nothing but closing punctuation is left of the word.
    fn runs_on(&self, rest: &[u8]) -> bool {
        match rest {
            [] | [b'/', ..] => false,
            [b'\\', ..] => !self.windows,
            _ => !rest
                .iter()
                .take_while(|byte| !byte.is_ascii_whitespace())
                .all(|byte| CLOSING_PUNCTUATION.contains(byte)),
        }
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
    fn roots_relative_and_unc_paths_are_not_homes() {
        for home in [
            "",
            "/",
            "//",
            r"C:\",
            "C:/",
            "home/alice",
            "~",
            r"\\server\share\alice",
            r"\\?\UNC\server\share\alice",
            r"\\?\C:\",
        ] {
            assert_eq!(HomePrefix::new(home), None, "{home}");
        }
    }

    #[test]
    fn verbatim_windows_home_is_read_in_its_ordinary_form() {
        let home = HomePrefix::new(r"\\?\C:\Users\alice").unwrap();
        assert_eq!(home, HomePrefix::new(r"C:\Users\alice").unwrap());
        assert_eq!(
            home.shorten(r"see C:\Users\alice\proj\x.rs"),
            r"see ~\proj\x.rs"
        );
        assert_eq!(
            home.expand(r"~\proj\x.rs"),
            Some(PathBuf::from(r"C:\Users\alice\proj\x.rs"))
        );
    }

    #[test]
    fn sibling_folders_sharing_the_home_name_are_left_whole() {
        for (home, separator, siblings) in [
            (
                "/home/alice",
                "/",
                [
                    "/home/alice+backup/x.rs",
                    "/home/alice@old/x.rs",
                    "/home/alice,v2/x.rs",
                    "/home/alice)x/x.rs",
                    "/home/alice.d/x.rs",
                    "/home/alice./x.rs",
                ],
            ),
            (
                r"C:\Users\alice",
                r"\",
                [
                    r"C:\Users\alice+backup\x.rs",
                    r"C:\Users\alice@old\x.rs",
                    r"C:\Users\alice,v2\x.rs",
                    r"C:\Users\alice)x\x.rs",
                    r"C:\Users\alice.d\x.rs",
                    r"C:\Users\alice.\x.rs",
                ],
            ),
        ] {
            let prefix = HomePrefix::new(home).unwrap();
            for sibling in siblings {
                let text = format!("open {sibling} now");
                assert_eq!(prefix.shorten(&text), text, "{sibling}");
            }
            let inside = format!("{home}{separator}proj{separator}x.rs");
            let shortened = prefix.shorten(&inside).into_owned();
            assert_eq!(shortened, format!("~{separator}proj{separator}x.rs"));
            assert_eq!(prefix.expand(&shortened), Some(PathBuf::from(&inside)));
            for (text, expected) in [
                (format!("({home}), then"), "(~), then"),
                (format!("\"{home}\"."), "\"~\"."),
                (format!("**{home}**"), "**~**"),
            ] {
                assert_eq!(prefix.shorten(&text), expected, "{text}");
            }
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
        for home in [r"C:\Users\alice", "C:/Users/alice/"] {
            let windows = HomePrefix::new(home).unwrap();
            for shortened in [r"~\proj\x.rs", "~/proj/x.rs", r"~/proj\x.rs"] {
                assert_eq!(
                    windows.expand(shortened),
                    Some(PathBuf::from(r"C:\Users\alice\proj\x.rs")),
                    "{home} {shortened}"
                );
            }
            assert_eq!(windows.expand("~"), Some(PathBuf::from(r"C:\Users\alice")));
        }
    }
}
