use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    engine::{SearchEngine, TestClock},
    tests::request,
    types::{
        SearchLimitReason, SearchLimits, SearchRoot, SearchRootSource, SearchRootState,
        WorkspaceSearchRequest, WorkspaceSearchResponse,
    },
};
use crate::{error::AppResult, workspace::Workspace};

fn tree() -> PathBuf {
    let path = std::env::temp_dir().join(format!("sage-search-listing-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(path.join("slow")).unwrap();
    fs::write(path.join("a.txt"), "needle\n").unwrap();
    for file in 0..60 {
        fs::write(path.join(format!("slow/f{file:02}.txt")), "needle\n").unwrap();
    }
    path.canonicalize().unwrap()
}

fn search(
    engine: &SearchEngine,
    path: &Path,
    token: Option<String>,
) -> AppResult<WorkspaceSearchResponse> {
    engine.search(
        WorkspaceSearchRequest {
            continuation_token: token,
            ..request("needle")
        },
        vec![SearchRoot {
            root_id: "root-test".into(),
            root_name: "fixture".into(),
            source: SearchRootSource::Available(
                Workspace::open("project-test", "root-test", path).unwrap(),
            ),
        }],
    )
}

fn slow_engine(path: &Path, threads: usize) -> SearchEngine {
    let mut engine = SearchEngine::new(SearchLimits::default());
    engine.threads = threads;
    // Listing the whole folder would take six seconds, past the helper's
    // run deadline. Only this folder advances time, independent of scheduling.
    engine.test_clock = Some(
        TestClock::new(Duration::ZERO)
            .with_slow_listing(path.join("slow"), Duration::from_millis(100)),
    );
    engine
}

#[test]
fn listing_deadline_keeps_matches_and_a_usable_continuation() {
    let path = tree();
    for threads in [1, 2] {
        let engine = slow_engine(&path, threads);
        let started = engine.now();
        let first = search(&engine, &path, None).unwrap();
        assert_eq!(
            engine.time_since(started),
            Duration::from_secs(2),
            "listing did not stop at the phase deadline with {threads} threads"
        );
        assert!(first.truncated);
        assert_eq!(first.limit_reasons, [SearchLimitReason::Time]);
        assert_eq!(first.roots[0].state, SearchRootState::Partial);
        assert_eq!(first.roots[0].files_scanned, 1);
        assert_eq!(first.roots[0].error_count, 0);
        let found = first
            .matches
            .iter()
            .map(|found| found.resource.relative_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(found, ["a.txt"]);
        let token = first
            .continuation_token
            .expect("partial matches can continue");

        let mut fast = SearchEngine::new(SearchLimits {
            max_duration: crate::test_wait::BUDGET,
            ..SearchLimits::default()
        });
        fast.threads = threads;
        let continued = search(&fast, &path, Some(token)).unwrap();
        assert!(!continued.truncated);
        assert!(continued.continuation_token.is_none());
        let found = continued
            .matches
            .into_iter()
            .map(|found| found.resource.relative_path)
            .collect::<Vec<_>>();
        let expected = (0..60)
            .map(|file| format!("slow/f{file:02}.txt"))
            .collect::<Vec<_>>();
        assert_eq!(found, expected);
    }
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn listing_deadline_bounds_continuation_replay() {
    let path = tree();
    let mut fast = SearchEngine::new(SearchLimits {
        max_results: 2,
        max_duration: crate::test_wait::BUDGET,
        ..SearchLimits::default()
    });
    fast.threads = 2;
    let first = search(&fast, &path, None).unwrap();
    assert_eq!(
        first.matches.last().unwrap().resource.relative_path,
        "slow/f00.txt"
    );
    let token = first.continuation_token.unwrap();
    for threads in [1, 2] {
        let engine = slow_engine(&path, threads);
        let started = engine.now();
        let error = search(&engine, &path, Some(token.clone())).unwrap_err();
        assert_eq!(
            engine.time_since(started),
            Duration::from_secs(2),
            "replay listing did not stop at the phase deadline with {threads} threads"
        );
        // A replay that cannot reach its cursor retains the existing stale
        // continuation response rather than inventing a completed page.
        assert!(
            error
                .to_string()
                .contains("workspace search continuation is stale")
        );
    }
    fs::remove_dir_all(path).unwrap();
}
