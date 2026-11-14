use std::{fs, path::Path, time::Duration};

use uuid::Uuid;

use super::{
    engine::{SearchEngine, TestClock},
    tests::request,
    types::{
        SearchLimitReason, SearchLimits, SearchRoot, SearchRootSource, WorkspaceSearchResponse,
    },
};
use crate::workspace::Workspace;

fn tree() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("sage-search-parallel-{}", Uuid::new_v4()));
    for folder in 0..12 {
        let folder_path = path.join(format!("pkg{folder:02}")).join("src");
        fs::create_dir_all(&folder_path).unwrap();
        for file in 0..9 {
            let body = match (folder + file) % 5 {
                0 => format!("fn needle_{file}() {{}}\nlet Needle = {folder};\n"),
                1 => "nothing to see here\n".repeat(40),
                2 => format!("// NEEDLE in a comment\r\nneedle needle {file}\n"),
                3 => "plain text without the word\n".into(),
                _ => format!("{}needle at the end\n", "x".repeat(200 * file)),
            };
            fs::write(folder_path.join(format!("file{file}.rs")), body).unwrap();
        }
    }
    fs::write(path.join("binary.bin"), b"needle\0\x89PNG").unwrap();
    fs::write(path.join("latin1.txt"), b"needle caf\xe9\n").unwrap();
    fs::write(path.join("large.txt"), "needle\n".repeat(2_000)).unwrap();
    fs::write(path.join(".gitignore"), "ignored/\n*.log\n").unwrap();
    fs::create_dir_all(path.join("ignored")).unwrap();
    fs::write(path.join("ignored").join("a.rs"), "needle\n").unwrap();
    fs::write(path.join("debug.log"), "needle\n").unwrap();
    path
}

fn search(
    path: &Path,
    threads: usize,
    limits: SearchLimits,
    query: &str,
    token: Option<String>,
) -> Result<WorkspaceSearchResponse, String> {
    search_with(path, threads, limits, query, token, None)
}

fn search_with(
    path: &Path,
    threads: usize,
    limits: SearchLimits,
    query: &str,
    token: Option<String>,
    time_checks: Option<u64>,
) -> Result<WorkspaceSearchResponse, String> {
    let mut engine = SearchEngine::new(limits);
    engine.threads = threads;
    engine.time_checks_left = time_checks.map(std::sync::atomic::AtomicU64::new);
    let mut request = request(query);
    request.continuation_token = token;
    let mut response = engine
        .search(
            request,
            vec![SearchRoot {
                root_id: "root-test".into(),
                root_name: "fixture".into(),
                source: SearchRootSource::Available(
                    Workspace::open("project-test", "root-test", path).unwrap(),
                ),
            }],
        )
        .map_err(|error| format!("{error:?}"))?;
    response.elapsed_ms = 0;
    Ok(response)
}

#[test]
fn four_threads_answer_exactly_as_one_does() {
    let path = tree();
    let base = SearchLimits {
        max_duration: sage_test_support::test_wait::BUDGET,
        ..SearchLimits::default()
    };
    let cases = [
        ("everything", base),
        (
            "few results",
            SearchLimits {
                max_results: 7,
                ..base
            },
        ),
        (
            "few bytes",
            SearchLimits {
                max_total_bytes: 3_000,
                ..base
            },
        ),
        (
            "small files only",
            SearchLimits {
                max_file_bytes: 900,
                ..base
            },
        ),
        (
            "few files",
            SearchLimits {
                max_files: 20,
                ..base
            },
        ),
        (
            "few entries",
            SearchLimits {
                max_entries: 40,
                ..base
            },
        ),
    ];
    assert_same_answers(&path, &cases);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn large_folders_answer_exactly_as_one_does() {
    let path = std::env::temp_dir().join(format!("sage-search-large-{}", Uuid::new_v4()));
    for (folder, count) in [("huge", 4_100), ("wide", 600)] {
        fs::create_dir_all(path.join(folder)).unwrap();
        for file in 0..count {
            let body = if file % 97 == 0 { "needle\n" } else { "x\n" };
            fs::write(path.join(folder).join(format!("f{file:05}.txt")), body).unwrap();
        }
    }
    let base = SearchLimits {
        max_duration: sage_test_support::test_wait::BUDGET,
        ..SearchLimits::default()
    };
    let cases = [
        ("everything", base),
        (
            "few results",
            SearchLimits {
                max_results: 7,
                ..base
            },
        ),
        (
            "few files",
            SearchLimits {
                max_files: 550,
                ..base
            },
        ),
    ];
    assert_same_answers(&path, &cases);
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn a_time_limit_stops_four_threads_where_it_stops_one() {
    let path = tree();
    let limits = SearchLimits {
        max_duration: sage_test_support::test_wait::BUDGET,
        max_results: 100_000,
        ..SearchLimits::default()
    };
    let mut stopped_with_matches = 0;
    // About 320 checks finish this tree, so this lands everywhere in it.
    for checks in 0..340 {
        let one = search_with(&path, 1, limits, "needle", None, Some(checks));
        let four = search_with(&path, 4, limits, "needle", None, Some(checks));
        assert_eq!(one, four, "time runs out after {checks} checks");
        let one = one.unwrap();
        if one.limit_reasons.contains(&SearchLimitReason::Time) && !one.matches.is_empty() {
            stopped_with_matches += 1;
        }
    }
    assert!(stopped_with_matches > 200, "{stopped_with_matches}");
    fs::remove_dir_all(path).unwrap();
}

fn assert_same_answers(path: &Path, cases: &[(&str, SearchLimits)]) {
    for (label, limits) in cases {
        for query in ["needle", "NEEDLE", "absent"] {
            let mut token = None;
            for page in 0..30 {
                let one = search(path, 1, *limits, query, token.clone());
                let four = search(path, 4, *limits, query, token.clone());
                assert_eq!(one, four, "{label}, query {query}, page {page}");
                let Ok(one) = one else {
                    break;
                };
                token = one.continuation_token;
                if token.is_none() {
                    break;
                }
            }
        }
    }
}

#[test]
fn a_line_with_more_matches_than_a_page_pages_through_all_of_them() {
    let path = std::env::temp_dir().join(format!("sage-search-dense-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    fs::write(
        path.join("dense.txt"),
        format!("{}\nNeedle\n", "é needle ".repeat(1_200)),
    )
    .unwrap();
    let limits = SearchLimits {
        max_duration: sage_test_support::test_wait::BUDGET,
        ..SearchLimits::default()
    };
    for threads in [1, 4] {
        let (mut columns, mut token, mut pages) = (Vec::new(), None, 0);
        loop {
            let page = search(&path, threads, limits, "NEEDLE", token).unwrap();
            pages += 1;
            columns.extend(
                page.matches
                    .iter()
                    .map(|found| (found.line_number, found.column_number)),
            );
            token = page.continuation_token;
            if token.is_none() {
                break;
            }
        }
        let expected = (0..1_200)
            .map(|index| (1, 3 + 9 * index))
            .chain([(2, 1)])
            .collect::<Vec<_>>();
        assert_eq!(columns, expected);
        assert_eq!(pages, 3);
    }
    fs::remove_dir_all(path).unwrap();
}

/// Time moves only as file types are read, and the page ends at its first
/// match, so a correct search reads at most two file types (the walk's and a
/// worker's) whichever thread runs first.
#[test]
fn slow_file_types_keep_the_first_match_before_the_time_limit() {
    let path = std::env::temp_dir().join(format!("sage-search-slow-types-{}", Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("a.txt"), "needle\n").unwrap();
    for file in 0..60 {
        fs::write(path.join(format!("b{file:02}.txt")), "needle\n").unwrap();
    }
    let limits = SearchLimits {
        max_duration: Duration::from_secs(1),
        max_results: 1,
        ..SearchLimits::default()
    };
    // Checking all 61 entries first takes three times the limit.
    for threads in [1, 4] {
        let mut engine = SearchEngine::new(limits);
        engine.threads = threads;
        engine.test_clock = Some(TestClock::new(Duration::from_millis(50)));
        let response = engine
            .search(
                request("needle"),
                vec![SearchRoot {
                    root_id: "root-test".into(),
                    root_name: "fixture".into(),
                    source: SearchRootSource::Available(
                        Workspace::open("project-test", "root-test", &path).unwrap(),
                    ),
                }],
            )
            .unwrap();
        assert_eq!(
            response.limit_reasons,
            [SearchLimitReason::Results],
            "{threads} threads"
        );
        let found = response
            .matches
            .iter()
            .map(|found| found.resource.relative_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(found, ["a.txt"], "{threads} threads");
    }
    fs::remove_dir_all(path).unwrap();
}
