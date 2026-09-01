use crate::error::{AppError, AppResult};

use super::types::{
    MAX_SEARCH_FILTER_BYTES, MAX_SEARCH_FILTER_PATTERN_BYTES, MAX_SEARCH_FILTER_PATTERNS,
};

pub(super) fn validate_patterns(includes: &[String], excludes: &[String]) -> AppResult<()> {
    if includes.len() > MAX_SEARCH_FILTER_PATTERNS || excludes.len() > MAX_SEARCH_FILTER_PATTERNS {
        return Err(invalid_filter("has too many patterns"));
    }
    let total_bytes = includes
        .iter()
        .chain(excludes)
        .map(String::len)
        .sum::<usize>();
    if total_bytes > MAX_SEARCH_FILTER_BYTES {
        return Err(invalid_filter("exceeds the total byte limit"));
    }
    for pattern in includes.iter().chain(excludes) {
        if !valid_pattern(pattern) {
            return Err(invalid_filter("contains an invalid pattern"));
        }
    }
    Ok(())
}

pub(super) fn path_is_admitted(path: &str, includes: &[String], excludes: &[String]) -> bool {
    (includes.is_empty() || includes.iter().any(|pattern| path_matches(pattern, path)))
        && !excludes.iter().any(|pattern| path_matches(pattern, path))
}

fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.len() <= MAX_SEARCH_FILTER_PATTERN_BYTES
        && !pattern.starts_with(['/', '~'])
        && !pattern.contains(['\\', ':', '\0'])
        && !pattern.chars().any(char::is_control)
        && pattern
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn invalid_filter(reason: &str) -> AppError {
    AppError::InvalidRequest(format!("workspace search path filter {reason}"))
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern_segments = pattern.split('/').collect::<Vec<_>>();
    let path_segments = path.split('/').collect::<Vec<_>>();
    let mut previous = vec![false; path_segments.len() + 1];
    previous[0] = true;
    for segment in pattern_segments {
        let mut current = vec![false; path_segments.len() + 1];
        if segment == "**" {
            current[0] = previous[0];
            for index in 1..current.len() {
                current[index] = previous[index] || current[index - 1];
            }
        } else {
            for index in 1..current.len() {
                current[index] =
                    previous[index - 1] && segment_matches(segment, path_segments[index - 1]);
            }
        }
        previous = current;
    }
    previous[path_segments.len()]
}

fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.chars().collect::<Vec<_>>();
    let value = value.chars().collect::<Vec<_>>();
    let (mut pattern_index, mut value_index, mut star_index, mut retry_index) = (0, 0, None, 0);
    while value_index < value.len() {
        if pattern.get(pattern_index) == Some(&'?')
            || pattern.get(pattern_index) == value.get(value_index)
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern.get(pattern_index) == Some(&'*') {
            star_index = Some(pattern_index);
            pattern_index += 1;
            retry_index = value_index;
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

#[cfg(test)]
mod tests {
    use super::{path_is_admitted, validate_patterns};

    #[test]
    fn matches_bounded_relative_globs() {
        assert!(path_is_admitted("src/main.rs", &[], &[]));
        assert!(path_is_admitted(
            "src/main.rs",
            &["src/**/*.rs".into()],
            &[]
        ));
        assert!(!path_is_admitted(
            "src/main.txt",
            &["src/**/*.rs".into()],
            &[]
        ));
        assert!(!path_is_admitted(
            "src/main.rs",
            &["**/*.rs".into()],
            &["**/main.rs".into()]
        ));
    }

    #[test]
    fn rejects_escape_and_control_patterns() {
        for pattern in ["../a", "/a", "C:/a", "a\\b", "a\nb"] {
            assert!(validate_patterns(&[pattern.into()], &[]).is_err());
        }
    }

    #[test]
    fn rejects_pattern_count_and_byte_overflow() {
        assert!(validate_patterns(&vec!["*.rs".into(); 17], &[]).is_err());
        assert!(validate_patterns(&["x".repeat(257)], &[]).is_err());
        assert!(validate_patterns(&vec!["x".repeat(256); 9], &[]).is_err());
    }
}
