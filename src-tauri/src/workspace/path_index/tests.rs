use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{domain::ResourceRef, workspace::notify::WorkspaceNotifyNotice};

use super::{
    IgnoreRules, IndexInputs, IndexedRoot, LATEST_GENERATION, PathGlobs, ProjectIndex,
    arena::PathArena,
    forget_project, note_change,
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

    fn notice(&self, relative: &str, kind: &str) {
        note_change(&WorkspaceNotifyNotice {
            project_id: self.project_id.clone(),
            project_generation: 1,
            transition_epoch: 1,
            root_id: "root-a".into(),
            relative_path: relative.into(),
            kind: kind.into(),
        });
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
}

/// Fixture projects have no file watcher; the tests deliver events
/// themselves, so they answer as if every root were watched.
fn ask(inputs: &IndexInputs, text: &str, recent: &[ResourceRef]) -> Answer {
    ask_watched(inputs, text, recent, true)
}

fn ask_watched(inputs: &IndexInputs, text: &str, recent: &[ResourceRef], watched: bool) -> Answer {
    let outcome = run(
        inputs,
        &QuickOpenQuery {
            text,
            limit: 50,
            generation: generation(),
            recent,
        },
        |_| watched,
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
    Answer {
        paths: results
            .iter()
            .map(|result| result.relative_path.clone())
            .collect(),
        results,
        files,
        built: readied.built,
    }
}

#[test]
fn indexes_what_search_would_scan() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&[
        "src/main.rs",
        "src/lib.rs",
        ".gitignore",
        "build/out.rs",
        "node_modules/pkg/index.js",
        ".git/config",
        "docs/notes.log",
    ]);
    fs::write(fixture.root.join(".gitignore"), "build/\n").unwrap();
    let inputs = fixture.inputs(&["**/*.log"]);
    let mut all = ask(&inputs, "", &[]).files;
    assert_eq!(all, 3, "src/main.rs, src/lib.rs and .gitignore");
    let answer = ask(&inputs, "rs", &[]);
    assert!(!answer.built, "the second query reuses the index");
    let mut paths = answer.paths.clone();
    paths.sort();
    assert_eq!(paths, ["src/lib.rs", "src/main.rs"]);
    all = ask(&fixture.inputs(&[]), "", &[]).files;
    assert_eq!(all, 4, "new files.exclude rebuilds, now with the .log file");
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
    assert!(ask(&inputs, "zzz", &[]).paths.is_empty());
}

#[test]
fn file_events_update_the_index_without_a_rebuild() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["src/old.rs", "src/keep.rs"]);
    let inputs = fixture.inputs(&[]);
    assert_eq!(ask(&inputs, "", &[]).files, 2);

    fs::remove_file(fixture.root.join("src/old.rs")).unwrap();
    fixture.write("src/new.rs");
    fixture.write("lib/deep/added.rs");
    fixture.notice("src/old.rs", "deleted");
    fixture.notice("src/new.rs", "created");
    fixture.notice("lib", "created");
    fixture.notice("src/keep.rs", "changed");
    fixture.notice(".git/index", "created");
    let answer = ask(&inputs, "rs", &[]);
    assert!(!answer.built);
    let mut paths = answer.paths;
    paths.sort();
    assert_eq!(paths, ["lib/deep/added.rs", "src/keep.rs", "src/new.rs"]);

    // A renamed folder arrives as its old name deleted and its new one created.
    fs::rename(fixture.root.join("lib"), fixture.root.join("pkg")).unwrap();
    fixture.notice("lib", "deleted");
    fixture.notice("pkg", "created");
    let mut paths = ask(&inputs, "added", &[]).paths;
    paths.sort();
    assert_eq!(paths, ["pkg/deep/added.rs"]);
}

#[test]
fn lost_events_and_ignore_file_changes_rebuild() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs", "tmp/b.rs"]);
    let inputs = fixture.inputs(&[]);
    assert_eq!(ask(&inputs, "", &[]).files, 2);

    fs::write(fixture.root.join(".gitignore"), "tmp/\n").unwrap();
    fixture.notice(".gitignore", "created");
    let answer = ask(&inputs, "rs", &[]);
    assert!(answer.built);
    assert_eq!(answer.paths, ["a.rs"]);

    fixture.write("c.rs");
    fixture.notice("lost-notices", "resync");
    let answer = ask(&inputs, "rs", &[]);
    assert!(answer.built);
    assert_eq!(answer.files, 3);
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
                recent: &[],
            },
            |_| true,
        )
    };
    assert!(matches!(ran(newer), Outcome::Ready { .. }));
    assert!(matches!(ran(older), Outcome::Superseded));
}

#[test]
fn forgetting_the_project_frees_the_index() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask(&inputs, "a", &[]).built);
    assert!(!ask(&inputs, "a", &[]).built);
    forget_project(&fixture.project_id);
    assert!(ask(&inputs, "a", &[]).built);
}

#[test]
fn an_index_without_a_watcher_is_rebuilt_until_watched() {
    let _serial = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let fixture = Fixture::new(&["a.rs"]);
    let inputs = fixture.inputs(&[]);
    assert!(ask_watched(&inputs, "rs", &[], false).built);
    // No event will say so, but the next query still sees the new File.
    fixture.write("b.rs");
    let unwatched = ask_watched(&inputs, "rs", &[], false);
    assert!(unwatched.built);
    assert_eq!(unwatched.files, 2);
    // Built unwatched, so the first watched query builds once more.
    assert!(ask(&inputs, "rs", &[]).built);
    assert!(!ask(&inputs, "rs", &[]).built);
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
fn matches_without_case_and_reports_character_positions() {
    let query = Query::new("ÉtUde").unwrap();
    assert_eq!(positions(&query, "dossier/étude.md", 8), [8, 9, 10, 11, 12]);
    let query = Query::new("src\\Main").unwrap();
    assert_eq!(
        positions(&query, "src/main.rs", 4),
        [0, 1, 2, 3, 4, 5, 6, 7]
    );
    assert!(score(&query, "src/main.rs", 4, true).is_some());
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
            arena.remove_under(&["src/mod3"]);
            IndexedRoot {
                root_id: root_id.into(),
                root: PathBuf::new(),
                arena,
                rules: IgnoreRules::load(&std::env::temp_dir(), false, None),
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
        excluded: PathGlobs::new(&[]),
        roots,
        watched: true,
    };
    let recent = [ResourceRef {
        root_id: "root-b".into(),
        relative_path: "src/mod5/item_5.rs".into(),
    }];
    for text in ["item5", "m1i2", "", "zz"] {
        let ranked = |shares| {
            rank_in_shares(
                &index,
                &QuickOpenQuery {
                    text,
                    limit: 20,
                    generation: u64::MAX,
                    recent: &recent,
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
