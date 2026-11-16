use super::{PathGlobs, path_glob_matches, valid_path_glob};

fn cases() -> serde_json::Value {
    serde_json::from_str(include_str!("cases.json")).unwrap()
}

#[test]
fn validity_matches_the_shared_cases() {
    let cases = cases();
    for pattern in cases["valid"].as_array().unwrap() {
        assert!(valid_path_glob(pattern.as_str().unwrap()), "{pattern}");
    }
    for pattern in cases["invalid"].as_array().unwrap() {
        assert!(!valid_path_glob(pattern.as_str().unwrap()), "{pattern}");
    }
    assert!(valid_path_glob(&"a".repeat(super::MAX_PATH_GLOB_BYTES)));
    assert!(!valid_path_glob(
        &"a".repeat(super::MAX_PATH_GLOB_BYTES + 1)
    ));
}

#[test]
fn single_patterns_and_compiled_lists_match_the_shared_cases() {
    for case in cases()["matches"].as_array().unwrap() {
        let pattern = case["pattern"].as_str().unwrap();
        let path = case["path"].as_str().unwrap();
        let expected = case["expected"].as_bool().unwrap();
        assert_eq!(path_glob_matches(pattern, path), expected, "{case}");
        assert_eq!(
            PathGlobs::new(&[pattern.into()]).matches(path),
            expected,
            "compiled {case}"
        );
    }
}

#[test]
fn a_compiled_list_matches_any_of_its_patterns_and_skips_invalid_ones() {
    let globs = PathGlobs::new(&[
        "**/.git".into(),
        "**/*.tmp".into(),
        "build/**".into(),
        "/absolute".into(),
    ]);
    assert!(globs.matches("x/.git"));
    assert!(globs.matches("a.tmp"));
    assert!(globs.matches("build/out/a.js"));
    assert!(!globs.matches("absolute"));
    assert!(!globs.matches("src/main.rs"));
    assert!(!PathGlobs::new(&[]).matches("anything"));
}
