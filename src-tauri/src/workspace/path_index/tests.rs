use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{domain::ResourceRef, workspace::search::MAX_SEARCH_DEPTH};

use super::{
    BUILDING, INDEX, IndexInputs, IndexedRoot, LATEST_GENERATION, ProjectIndex, WALKER_WAIT,
    arena::PathArena,
    build_index_within, focus_project, forget_project, lock, on_walker,
    query::{Outcome, QuickOpenQuery, Ranked, rank_in_shares, run},
    score::{Query, positions, score},
};

// The index is one per process, so these tests take turns.
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1_000_000);

fn generation() -> u64 {
    let next = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
    next.max(LATEST_GENERATION.load(Ordering::Acquire) + 1)
}

struct Fixture {
    root: PathBuf,
    project_id: String,
}

impl Fixture {
    fn new(files: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!("sage-path-index-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let fixture = Self {
            root: root.canonicalize().unwrap(),
            project_id: format!("project-{}", uuid::Uuid::new_v4()),
        };
        for file in files {
            fixture.write(file);
        }
        fixture
    }

    fn write(&self, relative: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, relative).unwrap();
    }

    fn inputs(&self, files_exclude: &[&str]) -> IndexInputs {
        IndexInputs {
            project_id: self.project_id.clone(),
            roots: vec![("root-a".into(), self.root.clone())],
            use_ignore_files: true,
            files_exclude: files_exclude.iter().map(|glob| (*glob).into()).collect(),
            global_excludes: None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        forget_project(&self.project_id);
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Answer {
    paths: Vec<String>,
    results: Vec<Ranked>,
    files: usize,
    built: bool,
    refreshing: bool,
}

fn ask(inputs: &IndexInputs, text: &str, recent: &[ResourceRef]) -> Answer {
    ask_in(inputs, 1, text, recent)
}

fn ask_in(inputs: &IndexInputs, session: u64, text: &str, recent: &[ResourceRef]) -> Answer {
    let outcome = run(
        inputs,
        &QuickOpenQuery {
            text,
            limit: 50,
            generation: generation(),
            session,
            recent,
            browse: false,
        },
        super::ticket(),
    );
    let Outcome::Ready {
        results,
        files,
        readied,
        ..
    } = outcome
    else {
        panic!("query was superseded");
    };
    let mut paths = results
        .iter()
        .map(|result| result.relative_path.clone())
        .collect::<Vec<_>>();
    if text.trim().is_empty() {
        paths.sort();
    }
    Answer {
        paths,
        results,
        files,
        built: readied.built,
        refreshing: readied.refreshing,
    }
}

/// Asks until the background refresh has been swapped in.
fn ask_refreshed(inputs: &IndexInputs, session: u64, text: &str) -> Answer {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let answer = ask_in(inputs, session, text, &[]);
        if !answer.refreshing {
            return answer;
        }
        assert!(Instant::now() < deadline, "the refresh never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn sorted(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths
}

#[test]
fn indexes_what_search_would_scan() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let deep = (0..=MAX_SEARCH_DEPTH)
        .map(|level| format!("d{level}"))
        .collect::<Vec<_>>()
        .join("/");
    let fixture = Fixture::new(&[
        "src/main.rs",
        "src/lib.rs",
        ".gitignore",
        "build/out.rs",
        "node_modules/pkg/index.js",
        ".git/config",
        "docs/notes.log",
        &format!("{deep}/too-deep.rs"),
    ]);
    fs::write(fixture.root.join(".gitignore"), "build/\n").unwrap();
    let inputs = fixture.inputs(&["**/*.log"]);
    let all = ask(&inputs, "", &[]);
    assert!(all.built);
    assert_eq!(all.files, 3, "src/main.rs, src/lib.rs and .gitignore");
    let answer = ask(&inputs, "rs", &[]);
    assert!(!answer.built, "the second query reuses the index");
    assert!(!answer.refreshing, "the same session doesn't refresh");
    assert_eq!(sorted(answer.paths), ["src/lib.rs", "src/main.rs"]);
    let rebuilt = ask(&fixture.inputs(&[]), "", &[]);
    assert!(rebuilt.built, "a new files.exclude rebuilds at once");
    assert_eq!(rebuilt.files, 4, "now with the .log file");
}

#[test]
fn ranks_basename_word_start_and_recent_matches_first() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&[
        "src/domain/mainline.ts",
        "src/main.ts",
        "src/app/remain.ts",
        "mains/other.ts",
        "README.md",
    ]);
    let inputs = fixture.inputs(&[]);
    let answer = ask(&inputs, "main", &[]);
    assert_eq!(answer.paths[0], "src/main.ts");
    assert_eq!(answer.paths.last().unwrap(), "mains/other.ts");
    assert_eq!(answer.results[0].positions, [4, 5, 6, 7]);
    let recent = [ResourceRef {
        root_id: "root-a".into(),
        relative_path: "src/app/remain.ts".into(),
    }];
    let boosted = ask(&inputs, "main", &recent);
    assert_eq!(boosted.paths[0], "src/app/remain.ts");
    let empty = ask(&inputs, "  ", &recent);
    assert_eq!(empty.paths, ["src/app/remain.ts"]);
    // Browsing lists the recent File first, then the rest, shortest first.
    let browsed = run(
        &inputs,
        &QuickOpenQuery {
            text: "",
            limit: 3,
            generation: generation(),
            session: 1,
            recent: &recent,
            browse: true,
        },
        super::ticket(),
    );
    let Outcome::Ready { results, .. } = browsed else {
        panic!("query was superseded");
    };
    let browsed: Vec<_> = results.iter().map(|r| r.relative_path.as_str()).collect();
    assert_eq!(browsed, ["src/app/remain.ts", "README.md", "src/main.ts"]);
    assert!(ask(&inputs, "zzz", &[]).paths.is_empty());
    let pieces = ask(&inputs, "main src", &[]);
    assert_eq!(pieces.paths[0], "src/main.ts", "pieces match in any order");
    assert!(!pieces.paths.contains(&"mains/other.ts".to_owned()));
}

#[test]
fn a_new_session_sees_changes_made_since_the_last() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["src/old.rs", "src/keep.rs", "tmp/scratch.rs"]);
    let inputs = fixture.inputs(&[]);
    assert_eq!(ask_in(&inputs, 1, "", &[]).files, 3);

    fs::remove_file(fixture.root.join("src/old.rs")).unwrap();
    fixture.write("lib/deep/added.rs");
    fs::rename(fixture.root.join("src"), fixture.root.join("pkg")).unwrap();
    fs::write(fixture.root.join(".gitignore"), "tmp/\n").unwrap();
    // The session that built the index keeps answering from it.
    let same = ask_in(&inputs, 1, "rs", &[]);
    assert!(!same.built && !same.refreshing);
    assert_eq!(same.paths.len(), 3);

    // A new session answers at once from what it has, then sees the changes.
    let first = ask_in(&inputs, 2, "rs", &[]);
    assert!(!first.built);
    let fresh = ask_refreshed(&inputs, 2, "rs");
    assert_eq!(sorted(fresh.paths), ["lib/deep/added.rs", "pkg/keep.rs"]);
    assert!(!ask_in(&inputs, 2, "rs", &[]).refreshing);
}

#[test]
fn a_newer_query_supersedes_an_older_one() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    let older = generation();
    let newer = generation();
    let ran = |generation| {
        run(
            &inputs,
            &QuickOpenQuery {
                text: "a",
                limit: 5,
                generation,
                session: 1,
                recent: &[],
                browse: false,
            },
            super::ticket(),
        )
    };
    assert!(matches!(ran(newer), Outcome::Ready { .. }));
    assert!(matches!(ran(older), Outcome::Superseded));
}

#[test]
fn forgetting_the_project_frees_the_index_without_waiting_for_a_walk() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask(&inputs, "a", &[]).built);
    assert!(!ask(&inputs, "a", &[]).built);
    forget_project("another-project");
    assert!(
        !ask(&inputs, "a", &[]).built,
        "only its own Project's index"
    );
    forget_project(&fixture.project_id);
    assert!(ask(&inputs, "a", &[]).built);

    // A first build doesn't hold the index lock while it walks, so
    // forgetting never waits for one.
    let building = lock(&BUILDING);
    let started = Instant::now();
    forget_project(&fixture.project_id);
    assert!(started.elapsed() < Duration::from_millis(500));
    drop(building);
}

#[test]
fn a_walk_that_ends_after_a_forget_keeps_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask_in(&inputs, 1, "a", &[]).built);
    // The refresh is queued behind a job that holds the walker thread until
    // the Project has been forgotten.
    let (release, wait) = std::sync::mpsc::channel::<()>();
    assert!(on_walker(Box::new(move || {
        let _ = wait.recv();
        false
    })));
    assert!(ask_in(&inputs, 2, "a", &[]).refreshing);
    forget_project(&fixture.project_id);
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while lock(&INDEX).refreshing {
        assert!(Instant::now() < deadline, "the refresh never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(lock(&INDEX).index.is_none());
}

#[test]
fn a_query_checked_before_its_project_closed_keeps_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    // The query passed the active-Project check, then the Project closed
    // while it waited for another build.
    let ticket = super::ticket();
    forget_project(&fixture.project_id);
    let query = QuickOpenQuery {
        text: "a",
        limit: 5,
        generation: generation(),
        session: 1,
        recent: &[],
        browse: false,
    };
    assert!(matches!(run(&inputs, &query, ticket), Outcome::Superseded));
    assert!(lock(&INDEX).index.is_none());
    // A query checked after the forget builds as usual.
    assert!(ask(&inputs, "a", &[]).built);
}

#[test]
fn a_first_build_stops_a_stale_refresh_and_outwaits_a_stuck_walker() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs", "b.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask_in(&inputs, 1, "rs", &[]).built);
    // The walker is held past `WALKER_WAIT`, with a refresh queued behind.
    let (release, wait) = std::sync::mpsc::channel::<()>();
    assert!(on_walker(Box::new(move || {
        let _ = wait.recv();
        false
    })));
    assert!(ask_in(&inputs, 2, "rs", &[]).refreshing);
    // New settings rebuild at once: on a fallback thread, since the walker
    // doesn't take the build up in time.
    let excluding = fixture.inputs(&["b.rs"]);
    let started = Instant::now();
    let rebuilt = ask_in(&excluding, 3, "rs", &[]);
    assert!(rebuilt.built);
    assert_eq!(rebuilt.paths, ["a.rs"]);
    assert!(started.elapsed() >= WALKER_WAIT);
    release.send(()).unwrap();
    // The stale refresh is stopped and leaves the new index in place.
    let deadline = Instant::now() + Duration::from_secs(10);
    while lock(&INDEX).refreshing {
        assert!(Instant::now() < deadline, "the refresh never finished");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(ask_in(&excluding, 3, "rs", &[]).paths, ["a.rs"]);
    assert!(lock(&INDEX).walks.is_empty());
}

#[test]
fn watching_another_project_drops_this_ones_index() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask(&inputs, "a", &[]).built);
    focus_project(&fixture.project_id);
    assert!(!ask(&inputs, "a", &[]).built, "its own Project keeps it");
    // A query checked before the switch keeps nothing afterwards.
    let ticket = super::ticket();
    focus_project("the-next-project");
    assert!(lock(&INDEX).index.is_none());
    let query = QuickOpenQuery {
        text: "a",
        limit: 5,
        generation: generation(),
        session: 1,
        recent: &[],
        browse: false,
    };
    assert!(matches!(run(&inputs, &query, ticket), Outcome::Superseded));
    assert!(lock(&INDEX).index.is_none());
}

#[test]
fn a_query_waiting_for_a_build_gives_up_once_superseded() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    let building = lock(&BUILDING);
    let older = generation();
    let waiting = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            let query = QuickOpenQuery {
                text: "a",
                limit: 5,
                generation: older,
                session: 1,
                recent: &[],
                browse: false,
            };
            run(&inputs, &query, super::ticket())
        });
        // Once it has claimed its generation it is waiting on the build.
        while LATEST_GENERATION.load(Ordering::Acquire) < older {
            std::thread::sleep(Duration::from_millis(1));
        }
        std::thread::sleep(Duration::from_millis(20));
        // A newer query arrives; the older one stops waiting.
        LATEST_GENERATION.fetch_max(generation(), Ordering::AcqRel);
        waiter.join().unwrap()
    });
    drop(building);
    assert!(matches!(waiting, Outcome::Superseded));
}

#[test]
fn a_stopped_walk_ends_early() {
    let fixture = Fixture::new(&["a.rs", "b.rs"]);
    let inputs = fixture.inputs(&[]);
    let index = build_index_within(&inputs, 1, None, 100, &AtomicBool::new(true));
    assert_eq!(index.files(), 0);
}

#[test]
fn roots_share_the_file_limit() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let large = Fixture::new(&["a1.rs", "a2.rs", "a3.rs", "a4.rs", "a5.rs", "a6.rs"]);
    let small = Fixture::new(&["b1.rs", "b2.rs"]);
    let mut inputs = large.inputs(&[]);
    inputs.roots.push(("root-b".into(), small.root.clone()));
    let index = build_index_within(&inputs, 1, None, 5, &AtomicBool::new(false));
    let counts = index
        .roots
        .iter()
        .map(|root| (root.arena.len(), root.truncated))
        .collect::<Vec<_>>();
    assert_eq!(counts, [(2, true), (2, false)]);
    assert!(index.truncated());
}

#[test]
fn scores_tight_word_start_matches_above_scattered_ones() {
    let query = Query::new("fb").unwrap();
    let mut ranked = ["foo/xfxb.ts", "fb.ts", "foo/bar.ts", "foobar.ts"]
        .into_iter()
        .map(|path| {
            let base = path.rfind('/').map_or(0, |slash| slash + 1);
            (score(&query, path, base, true).unwrap(), path)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    let order = ranked.iter().map(|(_, path)| *path).collect::<Vec<_>>();
    assert_eq!(order[0], "fb.ts");
    assert_eq!(order.last(), Some(&"foo/xfxb.ts"));
    assert_eq!(score(&query, "abc.ts", 0, true), None);
}

#[test]
fn matches_without_case_and_reports_utf16_positions() {
    let query = Query::new("ÉtUde").unwrap();
    assert_eq!(positions(&query, "dossier/étude.md", 8), [8, 9, 10, 11, 12]);
    // The emoji is two UTF-16 units, so the matches after it shift by one.
    let query = Query::new("ab").unwrap();
    let path = "x/😀ab.md";
    assert_eq!(positions(&query, path, 2), [4, 5]);
    let query = Query::new("src\\Main").unwrap();
    assert_eq!(
        positions(&query, "src/main.rs", 4),
        [0, 1, 2, 3, 4, 5, 6, 7]
    );
    assert!(score(&query, "src/main.rs", 4, true).is_some());
    let query = Query::new("rs main").unwrap();
    assert_eq!(positions(&query, "src/main.rs", 4), [4, 5, 6, 7, 9, 10]);
}

#[test]
fn ranking_split_across_threads_matches_one_thread() {
    let roots = ["root-a", "root-b"]
        .into_iter()
        .map(|root_id| {
            let mut arena = PathArena::default();
            for index in 0..6000 {
                arena.push(&format!("src/mod{}/item_{index}.rs", index % 37));
            }
            IndexedRoot {
                root_id: root_id.into(),
                arena,
                truncated: false,
            }
        })
        .collect();
    let index = ProjectIndex {
        inputs: IndexInputs {
            project_id: "split".into(),
            roots: Vec::new(),
            use_ignore_files: false,
            files_exclude: Vec::new(),
            global_excludes: None,
        },
        roots,
        session: 1,
    };
    let recent = [ResourceRef {
        root_id: "root-b".into(),
        relative_path: "src/mod5/item_5.rs".into(),
    }];
    for text in ["item5", "m1i2", "item mod3", "", "zz"] {
        let ranked = |shares| {
            rank_in_shares(
                &index,
                &QuickOpenQuery {
                    text,
                    limit: 20,
                    generation: u64::MAX,
                    session: 1,
                    recent: &recent,
                    browse: false,
                },
                shares,
            )
            .unwrap()
            .into_iter()
            .map(|result| (result.root_id, result.relative_path, result.positions))
            .collect::<Vec<_>>()
        };
        let alone = ranked(1);
        assert_eq!(alone, ranked(4), "query {text:?}");
        assert_eq!(alone.is_empty(), text == "zz");
    }
}
