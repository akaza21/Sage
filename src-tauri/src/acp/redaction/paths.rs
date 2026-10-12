#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathMode {
    Prose,
    CommandOutput,
    Permission,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathKind {
    Posix,
    Windows,
}

#[derive(Default)]
pub(crate) struct PathContext {
    windows: bool,
    home_name: bool,
}

const HOME_PREFIXES: &[&str] = &["\\users\\", "\\home\\", "/users/", "/home/"];

impl PathContext {
    pub(crate) fn redact(&mut self, body: &str, whitespace: &str, mode: PathMode) -> bool {
        let normalized = normalized_slashes(body);
        let body = normalized.as_ref();
        let path = detect_absolute_path(body, mode);
        let continuation =
            (self.home_name && body.contains(['\\', '/'])) || (self.windows && body.contains('\\'));
        if !body.is_empty() {
            if path.is_some() {
                let lower = body.to_ascii_lowercase();
                self.home_name = HOME_PREFIXES.iter().any(|prefix| {
                    lower
                        .find(prefix)
                        .is_some_and(|index| !lower[index + prefix.len()..].contains(['\\', '/']))
                });
            } else if self.home_name && body.contains(['\\', '/']) {
                self.home_name = false;
            }
            self.windows = path == Some(PathKind::Windows) || continuation;
        }
        if whitespace.contains(['\n', '\r']) || body.contains([';', '|', '&']) {
            self.windows = false;
            self.home_name = false;
        }
        path.is_some() || continuation
    }

    pub(crate) fn end_span(&mut self, span: &str) {
        if span.contains([';', '|', '&', '\n', '\r']) {
            self.windows = false;
            self.home_name = false;
        }
    }
}

pub(crate) fn absolute_path(value: &str, mode: PathMode) -> Option<PathKind> {
    detect_absolute_path(&normalized_slashes(value), mode)
}

fn normalized_slashes(value: &str) -> std::borrow::Cow<'_, str> {
    if !value.as_bytes().windows(2).any(|pair| pair == b"\\/") {
        return std::borrow::Cow::Borrowed(value);
    }
    let mut normalized = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' || chars.peek() != Some(&'/') {
            normalized.push(character);
        }
    }
    std::borrow::Cow::Owned(normalized)
}

fn detect_absolute_path(value: &str, mode: PathMode) -> Option<PathKind> {
    let bytes = value.as_bytes();
    if bytes.windows(7).enumerate().any(|(index, prefix)| {
        prefix.eq_ignore_ascii_case(b"file://")
            && (mode != PathMode::Prose
                || bytes[index + 7..].starts_with(b"/")
                || bytes[index + 7..].starts_with(b"\\\\"))
    }) {
        return Some(PathKind::Posix);
    }
    let last_non_slash = bytes.iter().rposition(|byte| *byte != b'/');
    let last_slash = bytes.iter().rposition(|byte| *byte == b'/');
    let flag_start = usize::from(matches!(bytes.first(), Some(b'"' | b'\'' | b'`')));
    let glued_path = if bytes.get(flag_start) == Some(&b'-') {
        let letters = bytes[flag_start + 1..]
            .iter()
            .take_while(|byte| byte.is_ascii_alphabetic())
            .count();
        (letters > 0).then_some(flag_start + 1 + letters)
    } else {
        None
    };
    let short_path = bytes.get(flag_start).is_some_and(|byte| *byte == b'-')
        && bytes
            .get(flag_start + 1)
            .is_some_and(u8::is_ascii_alphabetic);
    for index in 0..bytes.len() {
        if !(glued_path == Some(index)
            || (glued_path == Some(index + 1) && bytes.get(index + 1) == Some(&b':')))
            && !(short_path && index == flag_start + 2)
            && !is_path_boundary(bytes, index)
        {
            continue;
        }
        let rest = &bytes[index..];
        if (index
            .checked_sub(1)
            .is_none_or(|previous| bytes[previous] != b'\\')
            && unc_path(rest))
            || (rest.len() >= 3
                && rest[0].is_ascii_alphabetic()
                && rest[1] == b':'
                && matches!(rest[2], b'\\' | b'/'))
        {
            return Some(PathKind::Windows);
        }
        if rest[0] != b'/' {
            continue;
        }
        let previous = index.checked_sub(1).map(|index| bytes[index]);
        let posix = match mode {
            PathMode::Permission => true,
            PathMode::CommandOutput | PathMode::Diagnostic => {
                rest != b"/*"
                    && (plain_slash_path(rest)
                        || (matches!(rest.get(1), Some(b'/' | b'*'))
                            && last_slash.is_some_and(|last| last > index + 1)))
                    && last_non_slash.is_some_and(|last| last >= index)
                    && (previous != Some(b'<') || !is_closing_tag(bytes, index))
            }
            PathMode::Prose => {
                plain_slash_path(rest)
                    && (previous != Some(b'<') || last_slash.is_some_and(|last| last > index))
            }
        };
        if posix && !(previous == Some(b':') && rest.starts_with(b"//")) {
            return Some(PathKind::Posix);
        }
    }
    None
}

fn is_path_boundary(value: &[u8], index: usize) -> bool {
    let previous = index.checked_sub(1).map(|index| value[index]);
    previous.is_none_or(|byte| {
        matches!(
            byte,
            b'"' | b'\''
                | b'`'
                | b'('
                | b'['
                | b'{'
                | b'<'
                | b'>'
                | b'|'
                | b';'
                | b'&'
                | b'='
                | b','
                | b':'
                | b'\\'
                | b'@'
        )
    })
}

fn plain_slash_path(rest: &[u8]) -> bool {
    rest.get(1).is_some_and(|byte| {
        !byte.is_ascii()
            || byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'_' | b'-' | b'~')
    })
}

fn is_closing_tag(value: &[u8], slash: usize) -> bool {
    let name_end = value[slash + 1..]
        .iter()
        .position(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'-' | b':'));
    name_end.is_some_and(|end| {
        end > 0
            && value.get(slash + end + 1) == Some(&b'>')
            && (slash + end + 2 == value.len()
                || matches!(value.get(slash + end + 2), Some(b'\'' | b'"')))
    })
}

fn unc_path(rest: &[u8]) -> bool {
    rest.starts_with(b"\\\\")
        && rest[2..]
            .iter()
            .find(|byte| **byte != b'\\')
            .is_none_or(|byte| !matches!(byte, b'"' | b'\'' | b'\x60'))
}
