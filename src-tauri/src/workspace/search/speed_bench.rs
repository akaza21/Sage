//! Manual timing of Search on a tree from `scripts/synthetic-tree.mjs`:
//! `SAGE_SEARCH_BENCH_ROOT=<tree> cargo test --release --lib speed_bench -- --ignored --nocapture`

use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use super::{
    engine::SearchEngine,
    tests::request,
    types::{SearchLimits, SearchRoot, SearchRootSource},
};
use crate::workspace::Workspace;

fn root(path: &Path) -> Vec<SearchRoot> {
    vec![SearchRoot {
        root_id: "root-test".into(),
        root_name: "bench".into(),
        source: SearchRootSource::Available(
            Workspace::open("project-test", "root-test", path).unwrap(),
        ),
    }]
}

fn time(label: &str, limits: &SearchLimits, query: &str, case_sensitive: bool, path: &Path) {
    let mut samples = Vec::new();
    let mut summary = String::new();
    for _ in 0..7 {
        let mut search = request(query);
        search.case_sensitive = case_sensitive;
        let started = Instant::now();
        let mut engine = SearchEngine::new(*limits);
        if let Ok(threads) = std::env::var("SAGE_SEARCH_BENCH_THREADS") {
            engine.threads = threads.parse().unwrap();
        }
        let mut response = engine.search(search, root(path)).unwrap();
        samples.push(started.elapsed().as_millis());
        response.elapsed_ms = 0;
        // Compares answers across builds: everything but the timing.
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(&format!("{response:?}"), &mut hasher);
        let digest = std::hash::Hasher::finish(&hasher);
        summary = format!(
            "matches={} files={} bytes={} reasons={:?} digest={digest:016x}",
            response.matches.len(),
            response.roots[0].files_scanned,
            response.roots[0].bytes_scanned,
            response.limit_reasons,
        );
    }
    samples.sort_unstable();
    // The process's peak resident memory so far, for the memory budget.
    let peak = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find(|line| line.starts_with("VmHWM:"))
                .map(|line| line.split_whitespace().nth(1).unwrap_or("?").to_owned())
        })
        .unwrap_or_else(|| "?".into());
    println!(
        "SEARCH_BENCH {label} p50={}ms min={}ms max={}ms {summary} peak_kib={peak}",
        samples[3], samples[0], samples[6]
    );
}

#[test]
#[ignore = "manual benchmark; needs SAGE_SEARCH_BENCH_ROOT"]
fn speed_bench() {
    let path = PathBuf::from(std::env::var("SAGE_SEARCH_BENCH_ROOT").unwrap());
    let lifted = SearchLimits {
        max_results: usize::MAX,
        max_total_bytes: u64::MAX,
        max_duration: std::time::Duration::from_secs(600),
        ..SearchLimits::default()
    };
    let default = SearchLimits::default();
    for (label, limits) in [("full", &lifted), ("default", &default)] {
        time(
            &format!("{label} needle-insensitive"),
            limits,
            "sage_synthetic_needle",
            false,
            &path,
        );
        time(
            &format!("{label} needle-sensitive"),
            limits,
            "SAGE_SYNTHETIC_NEEDLE",
            true,
            &path,
        );
        time(
            &format!("{label} absent-insensitive"),
            limits,
            "zqxjv",
            false,
            &path,
        );
    }
}

/// Manual timing of the worst inputs for case-insensitive matching: long
/// single-line Files of one repeated character.
/// `cargo test --release --lib repetitive_bench -- --ignored --nocapture`
#[test]
#[ignore = "manual benchmark"]
fn repetitive_bench() {
    let path =
        std::env::temp_dir().join(format!("sage-search-repetitive-{}", uuid::Uuid::new_v4()));
    for (folder, character) in [("ascii", "a"), ("accented", "é")] {
        std::fs::create_dir_all(path.join(folder)).unwrap();
        let line = character.repeat(2 * 1024 * 1024 / character.len());
        for file in 0..8 {
            std::fs::write(path.join(folder).join(format!("{file}.txt")), &line).unwrap();
        }
    }
    let limits = SearchLimits {
        max_duration: std::time::Duration::from_secs(600),
        ..SearchLimits::default()
    };
    let near_miss = format!("{}b", "a".repeat(511));
    time("repetitive near-miss", &limits, &near_miss, false, &path);
    time("repetitive accented", &limits, "É", false, &path);
    // Matches nothing, so every File goes through non-ASCII case folding.
    time("repetitive folding miss", &limits, "Ë", false, &path);
    std::fs::remove_dir_all(path).unwrap();
}
