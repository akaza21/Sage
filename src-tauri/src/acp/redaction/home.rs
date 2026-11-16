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

/// Longest file name most filesystems allow, which bounds how far a home
/// folder's name can run on past a space.
const NAME_MAX: usize = 255;

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

    /// Whether words after a space may still be the home folder's name, as in
    /// `Alice Smith\project`: a later word on the line, within one name's
    /// length, reaches a separator before the clause or a new path begins.
    fn name_continues_after_space(&self, rest: &[u8]) -> bool {
        let line = &rest[..rest
            .iter()
            .position(|&byte| byte == b'\n')
            .unwrap_or(rest.len())];
        let line = &line[..line.len().min(NAME_MAX)];
        let mut words = line.split(|&byte| matches!(byte, b' ' | b'\t'));
        words.next();
        for word in words.filter(|word| !word.is_empty()) {
            if self.starts_path(word) {
                return false;
            }
            match word.iter().position(|&byte| self.is_separator(byte)) {
                Some(_) => return true,
                None if word
                    .last()
                    .is_some_and(|byte| CLOSING_PUNCTUATION.contains(byte)) =>
                {
                    return false;
                }
                None => {}
            }
        }
        false
    }

    /// Whether a word opens a new absolute path rather than continuing a name.
    fn starts_path(&self, word: &[u8]) -> bool {
        let start = word
            .iter()
            .position(|byte| !b"\"'`([{<".contains(byte))
            .unwrap_or(word.len());
        match &word[start..] {
            [first, ..] if self.is_separator(*first) => true,
            [drive, b':', separator, ..] => {
                self.windows && drive.is_ascii_alphabetic() && self.is_separator(*separator)
            }
            _ => false,
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
    fn siblings_whose_names_continue_past_a_space_are_left_whole() {
        for (home, separator) in [(r"C:\Users\Alice", r"\"), ("/home/alice", "/")] {
            let prefix = HomePrefix::new(home).unwrap();
            let sibling = format!("{home} Smith{separator}project{separator}file.rs");
            let long_sibling = format!("{home} Jane Smith{separator}file.rs");
            for text in [
                format!("open {sibling} now"),
                format!("\"{sibling}\""),
                format!("`{sibling}`"),
                format!("open {long_sibling} now"),
                format!("open `{home} Smith` now"),
                format!("open \"{home} Smith\" now"),
                format!("'{home} Smith'"),
            ] {
                assert_eq!(prefix.shorten(&text), text, "{text}");
            }
            for (text, expected) in [
                (format!("\"{home}\""), "\"~\"".to_owned()),
                (format!("`{home}`"), "`~`".to_owned()),
                (
                    format!("`{home}{separator}x.rs`"),
                    format!("`~{separator}x.rs`"),
                ),
                (format!("in {home}."), "in ~.".to_owned()),
                (format!("{home} is home"), "~ is home".to_owned()),
                (format!("{home}, then"), "~, then".to_owned()),
                (
                    format!("{home} then {home}{separator}x"),
                    format!("~ then ~{separator}x"),
                ),
                (
                    format!("{home}\nSmith{separator}x"),
                    format!("~\nSmith{separator}x"),
                ),
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
