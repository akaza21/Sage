use crate::{
    error::{AppError, AppResult},
    workspace::path_glob::{path_glob_matches, valid_path_glob},
};

use super::types::{MAX_SEARCH_FILTER_BYTES, MAX_SEARCH_FILTER_PATTERNS};

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
        if !valid_path_glob(pattern) {
            return Err(invalid_filter("contains an invalid pattern"));
        }
    }
    Ok(())
}

pub(super) fn path_is_admitted(path: &str, includes: &[String], excludes: &[String]) -> bool {
    (includes.is_empty()
        || includes
            .iter()
            .any(|pattern| path_glob_matches(pattern, path)))
        && !excludes
            .iter()
            .any(|pattern| path_glob_matches(pattern, path))
}

fn invalid_filter(reason: &str) -> AppError {
    AppError::InvalidRequest(format!("workspace search path filter {reason}"))
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
