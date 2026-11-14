//! Mirrored in src/domain/pathGlob.ts.

pub(crate) const MAX_PATH_GLOB_BYTES: usize = 256;

pub(crate) fn valid_path_glob(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= MAX_PATH_GLOB_BYTES
        && !pattern.starts_with(['/', '~'])
        && !pattern.contains(['\\', ':', '\0'])
        && !pattern.chars().any(char::is_control)
        && pattern
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

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

#[derive(Debug, Default)]
pub(crate) struct PathGlobs {
    names: Vec<String>,
    suffixes: Vec<String>,
    others: Vec<Segments>,
}

impl PathGlobs {
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
