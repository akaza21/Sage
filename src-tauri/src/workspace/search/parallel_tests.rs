//! Reading Files ahead on worker threads must not change any response: the
//! same tree, query and limits give byte-for-byte the same matches, reports,
//! limit reasons and continuation tokens with one thread as with four.

use std::{fs, path::Path};

use uuid::Uuid;

use super::{
    engine::SearchEngine,
    tests::request,
    types::{SearchLimits, SearchRoot, SearchRootSource, WorkspaceSearchResponse},
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
    let mut engine = SearchEngine::new(limits);
    engine.threads = threads;
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
        max_duration: crate::test_wait::BUDGET,
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

/// Folders too large to list or read ahead in one piece are left, wholly or
/// in part, to the walk.
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
        max_duration: crate::test_wait::BUDGET,
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

fn assert_same_answers(path: &Path, cases: &[(&str, SearchLimits)]) {
    for (label, limits) in cases {
        for query in ["needle", "NEEDLE", "absent"] {
            let mut token = None;
            for page in 0..30 {
                let one = search(path, 1, *limits, query, token.clone());
                let four = search(path, 4, *limits, query, token.clone());
                assert_eq!(one, four, "{label}, query {query}, page {page}");
                // A page can only end where a stale continuation is refused,
                // which both engines must agree on too.
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
