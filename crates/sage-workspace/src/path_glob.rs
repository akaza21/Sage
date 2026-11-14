//! Sage's path glob dialect, used by Search's include and exclude fields and
//! by the `files.exclude` setting. A pattern matches a whole Project-relative
//! path with `/` separators: a `**` segment matches any number of segments,
//! `*` any run of characters within a segment and `?` one character. Matching
//! is case-sensitive. Mirrored in src/domain/pathGlob.ts.

pub const MAX_PATH_GLOB_BYTES: usize = 256;
/// The most patterns a glob list (a `Globs` setting) holds.
pub const MAX_GLOBS: usize = 50;

/// The `files.exclude` setting's registry default.
pub const DEFAULT_FILES_EXCLUDE: &[&str] = &[
    "**/.git",
    "**/.svn",
    "**/.hg",
    "**/CVS",
    "**/.DS_Store",
    "**/Thumbs.db",
];

/// The `files.exclude` patterns a Search uses when its request names none.
pub fn default_files_exclude() -> Vec<String> {
    DEFAULT_FILES_EXCLUDE
        .iter()
        .map(|pattern| (*pattern).into())
        .collect()
}

/// Relative, bounded, without control characters, drive letters, backslashes
/// or empty, `.` and `..` segments.
pub fn valid_path_glob(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= MAX_PATH_GLOB_BYTES
        && !pattern.starts_with(['/', '~'])
        && !pattern.contains(['\\', ':', '\0'])
        && !pattern.chars().any(char::is_control)
        && pattern
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

/// A pattern split once into segments: `None` for `**`, else its characters.
type Segments = Vec<Option<Vec<char>>>;

fn segments(text: &str) -> Segments {
    text.split('/')
        .map(|segment| (segment != "**").then(|| segment.chars().collect()))
        .collect()
}

pub(crate) fn path_glob_matches(pattern: &str, path: &str) -> bool {
    segments_match(&segments(pattern), &path.split('/').collect::<Vec<_>>())
}

fn segments_match(pattern: &Segments, path: &[&str]) -> bool {
    let mut previous = vec![false; path.len() + 1];
    let mut current = vec![false; path.len() + 1];
    previous[0] = true;
    for segment in pattern {
        current[0] = segment.is_none() && previous[0];
        for index in 1..current.len() {
            current[index] = match segment {
                None => previous[index] || current[index - 1],
                Some(characters) => {
                    previous[index - 1] && segment_matches(characters, path[index - 1])
                }
            };
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[path.len()]
}

/// ASCII names, the common case, are matched without collecting characters.
fn segment_matches(pattern: &[char], value: &str) -> bool {
    if value.is_ascii() {
        wildcard_matches(pattern, value.as_bytes())
    } else {
        wildcard_matches(pattern, &value.chars().collect::<Vec<_>>())
    }
}

fn wildcard_matches<T: Copy>(pattern: &[char], value: &[T]) -> bool
where
    char: From<T>,
{
    let (mut pattern_index, mut value_index, mut star_index, mut retry_index) = (0, 0, None, 0);
    // `*` is tried before a literal match, so a name containing `*` still
    // records the backtrack point.
    while value_index < value.len() {
        let next = pattern.get(pattern_index).copied();
        if next == Some('*') {
            star_index = Some(pattern_index);
            pattern_index += 1;
            retry_index = value_index;
        } else if next == Some('?') || next == value.get(value_index).map(|&c| char::from(c)) {
            pattern_index += 1;
            value_index += 1;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            retry_index += 1;
            value_index = retry_index;
        } else {
            return false;
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

/// A list of patterns sorted once for a walk. The common shapes `**/name` and
/// `**/*.ext` are a comparison of the last segment, so a walk does not run
/// the general matcher for each entry and pattern.
#[derive(Debug, Default)]
pub(crate) struct PathGlobs {
    names: Vec<String>,
    suffixes: Vec<String>,
    others: Vec<Segments>,
}

impl PathGlobs {
    /// Invalid patterns are left out and logged; callers admit patterns first.
    pub(crate) fn new(patterns: &[String]) -> Self {
        let mut globs = Self::default();
        for pattern in patterns {
            if !valid_path_glob(pattern) {
                tracing::warn!(bytes = pattern.len(), "invalid path glob skipped");
                continue;
            }
            let last = pattern
                .strip_prefix("**/")
                .filter(|rest| !rest.contains('/'));
            let wild = |text: &str| text.contains(['*', '?']);
            match last {
                Some(name) if !wild(name) => globs.names.push(name.into()),
                Some(name) if name.starts_with('*') && !wild(&name[1..]) => {
                    globs.suffixes.push(name[1..].into())
                }
                _ => globs.others.push(segments(pattern)),
            }
        }
        globs
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        if self.names.iter().any(|candidate| candidate == name)
            || self
                .suffixes
                .iter()
                .any(|suffix| name.ends_with(suffix.as_str()))
        {
            return true;
        }
        // A pattern's first and last segments must match the path's, which
        // rules out most entries before the full match; the path is split
        // once for the patterns left.
        let first = path.split('/').next().unwrap_or(path);
        let mut segments: Option<Vec<&str>> = None;
        self.others.iter().any(|pattern| {
            let ends_fit = |end: Option<&Option<Vec<char>>>, part: &str| match end {
                Some(Some(characters)) => segment_matches(characters, part),
                _ => true,
            };
            ends_fit(pattern.first(), first)
                && ends_fit(pattern.last(), name)
                && segments_match(
                    pattern,
                    segments.get_or_insert_with(|| path.split('/').collect()),
                )
        })
    }
}

#[cfg(test)]
mod tests;
