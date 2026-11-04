use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    Cancellation, VaultError, VaultResult,
    cipher::{Binding, VaultKey},
    key_store::KeyProtector,
    secret::Secret,
    state::{PasswordsOverview, PasswordsState},
    store::{MergeCounts, NewCredential, Source, Store},
};

/// Wraps a key by prefixing the account it belongs to, so a key file from
/// another "account" fails to unwrap as DPAPI's would.
struct TestProtector(u8);

impl KeyProtector for TestProtector {
    fn protect(&self, key: &[u8]) -> io::Result<Vec<u8>> {
        Ok([&[self.0], key].concat())
    }

    fn unprotect(&self, wrapped: &[u8]) -> VaultResult<Zeroizing<Vec<u8>>> {
        match wrapped.split_first() {
            Some((account, key)) if *account == self.0 => Ok(Zeroizing::new(key.to_vec())),
            _ => Err(VaultError::KeyLost),
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("sage-passwords-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn csv(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn state(directory: &Path, account: u8) -> PasswordsState {
    PasswordsState::for_test(directory, Arc::new(TestProtector(account)))
}

fn credential(origin: &str, username: &str, password: &str) -> NewCredential {
    NewCredential {
        origin: origin.into(),
        username: username.into(),
        password: Secret::new(password.into()),
    }
}

fn import(state: &PasswordsState, path: PathBuf) -> MergeCounts {
    state.begin_import().unwrap().import(path).unwrap().0
}

fn merge(store: &mut Store, key: &VaultKey, credentials: &[NewCredential]) -> MergeCounts {
    store
        .merge(key, Source::Csv, credentials, &AtomicBool::new(false))
        .unwrap()
}

fn ready(overview: PasswordsOverview) -> super::store::CredentialPage {
    match overview {
        PasswordsOverview::Ready(page) => page,
        PasswordsOverview::Unsupported => panic!("vault unsupported"),
    }
}

const CHROME_CSV: &str = "name,url,username,password,note\n\
    example.com,https://example.com/login,ada,first-secret,\n\
    shop,https://shop.example:8443/,grace,second-secret,\n";

/// A later export: other passwords, and a different length.
const NEWER_CSV: &str = "name,url,username,password,note\n\
    example.com,https://example.com/login,ada,a-much-newer-secret,\n";

#[test]
fn a_sealed_password_opens_only_under_its_own_row_site_and_username() {
    let key = VaultKey::generate().unwrap();
    let binding = Binding {
        id: "row-1",
        origin: "https://example.com",
        username: "ada",
    };
    let sealed = key.seal(&binding, &Secret::new("hunter2".into())).unwrap();
    assert_eq!(key.open(&binding, &sealed).unwrap().expose(), "hunter2");
    assert!(!sealed.windows(7).any(|window| window == b"hunter2"));
    for moved in [
        Binding {
            id: "row-2",
            ..binding
        },
        Binding {
            origin: "https://evil.example",
            ..binding
        },
        Binding {
            username: "grace",
            ..binding
        },
    ] {
        assert!(matches!(
            key.open(&moved, &sealed),
            Err(VaultError::Corrupt)
        ));
    }
    let mut tampered = sealed.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(matches!(
        key.open(&binding, &tampered),
        Err(VaultError::Corrupt)
    ));
    let other = VaultKey::generate().unwrap();
    assert!(matches!(
        other.open(&binding, &sealed),
        Err(VaultError::Corrupt)
    ));
}

#[test]
fn merging_adds_new_rows_and_lets_the_newer_password_win() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    let first = [
        credential("https://example.com", "ada", "one"),
        credential("https://example.com", "grace", "two"),
    ];
    assert_eq!(
        merge(&mut store, &key, &first),
        MergeCounts {
            added: 2,
            updated: 0,
            unchanged: 0
        }
    );
    let second = [
        credential("https://example.com", "ada", "one"),
        credential("https://example.com", "grace", "three"),
    ];
    assert_eq!(
        merge(&mut store, &key, &second),
        MergeCounts {
            added: 0,
            updated: 1,
            unchanged: 1
        }
    );
    let page = store.page("").unwrap();
    assert_eq!((page.total, page.matching), (2, 2));
    let grace = page
        .entries
        .iter()
        .find(|entry| entry.username == "grace")
        .unwrap();
    assert_eq!(store.password(&key, &grace.id).unwrap().expose(), "three");
}

#[test]
fn search_matches_site_or_username_and_the_list_stays_capped() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    let many: Vec<_> = (0..super::store::MAX_LISTED + 5)
        .map(|index| credential(&format!("https://site{index}.example"), "user", "pw"))
        .chain([credential("https://mail.example", "Ada.Lovelace", "pw")])
        .collect();
    merge(&mut store, &key, &many);
    let all = store.page("").unwrap();
    assert_eq!(all.total, super::store::MAX_LISTED + 6);
    assert_eq!(all.matching, all.total);
    assert_eq!(all.entries.len(), super::store::MAX_LISTED as usize);
    let by_username = store.page("  ada.LOVE ").unwrap();
    assert_eq!(by_username.matching, 1);
    assert_eq!(by_username.entries[0].origin, "https://mail.example");
    assert_eq!(store.page("site12.").unwrap().matching, 1);
    assert_eq!(store.page("nothing").unwrap().entries, []);
}

#[test]
fn editing_a_username_reseals_and_refuses_a_taken_one() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    merge(
        &mut store,
        &key,
        &[
            credential("https://example.com", "ada", "one"),
            credential("https://example.com", "grace", "two"),
        ],
    );
    let page = store.page("").unwrap();
    let ada = page
        .entries
        .iter()
        .find(|entry| entry.username == "ada")
        .unwrap();
    store
        .update(&key, &ada.id, "ada@example.com", None)
        .unwrap();
    assert_eq!(store.password(&key, &ada.id).unwrap().expose(), "one");
    store
        .update(
            &key,
            &ada.id,
            "ada@example.com",
            Some(&Secret::new("new".into())),
        )
        .unwrap();
    assert_eq!(store.password(&key, &ada.id).unwrap().expose(), "new");
    assert!(matches!(
        store.update(&key, &ada.id, "grace", None),
        Err(VaultError::Duplicate)
    ));
    assert_eq!(store.password(&key, &ada.id).unwrap().expose(), "new");
    store.delete(&ada.id).unwrap();
    assert!(matches!(store.delete(&ada.id), Err(VaultError::NotFound)));
    assert!(matches!(
        store.password(&key, &ada.id),
        Err(VaultError::NotFound)
    ));
}

#[cfg(not(windows))]
#[test]
fn the_vault_is_unsupported_without_an_os_key_store() {
    let scratch = Scratch::new();
    let state = PasswordsState::new(&scratch.0);
    assert_eq!(state.overview("").unwrap(), PasswordsOverview::Unsupported);
    assert!(matches!(state.delete_all(), Err(VaultError::Unsupported)));
    assert!(fs::read_dir(&scratch.0).unwrap().next().is_none());
}

#[test]
fn importing_a_csv_stores_only_sealed_passwords_and_a_wrapped_key() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let counts = import(&state, scratch.csv("passwords.csv", CHROME_CSV));
    assert_eq!(
        counts,
        MergeCounts {
            added: 2,
            updated: 0,
            unchanged: 0
        }
    );
    let page = ready(state.overview("").unwrap());
    let origins: Vec<_> = page
        .entries
        .iter()
        .map(|entry| entry.origin.as_str())
        .collect();
    assert_eq!(
        origins,
        ["https://example.com", "https://shop.example:8443"]
    );
    for file in ["passwords.db", "passwords.key"] {
        let bytes = fs::read(scratch.0.join("profile").join(file)).unwrap();
        for secret in [&b"first-secret"[..], b"second-secret"] {
            assert!(
                !bytes.windows(secret.len()).any(|window| window == secret),
                "{file}"
            );
        }
    }
    assert_eq!(
        state.password(&page.entries[0].id).unwrap().expose(),
        "first-secret"
    );
}

#[test]
fn a_lost_or_foreign_key_is_reported_and_delete_all_starts_again() {
    let scratch = Scratch::new();
    let directory = scratch.0.join("profile");
    let id = {
        let state = state(&directory, 1);
        import(&state, scratch.csv("a.csv", CHROME_CSV));
        ready(state.overview("").unwrap()).entries[0].id.clone()
    };
    let foreign = state(&directory, 2);
    assert!(matches!(foreign.password(&id), Err(VaultError::KeyLost)));
    fs::remove_file(directory.join("passwords.key")).unwrap();
    let lost = state(&directory, 1);
    assert!(matches!(lost.password(&id), Err(VaultError::KeyLost)));
    assert!(matches!(
        lost.begin_import()
            .and_then(|run| run.import(scratch.csv("b.csv", CHROME_CSV))),
        Err(VaultError::KeyLost)
    ));
    lost.delete_all().unwrap();
    assert_eq!(ready(lost.overview("").unwrap()).total, 0);
    assert_eq!(
        import(&lost, scratch.csv("c.csv", CHROME_CSV)),
        MergeCounts {
            added: 2,
            updated: 0,
            unchanged: 0
        }
    );
}

#[test]
fn a_csv_saved_over_the_imported_one_is_not_deleted() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let csv = scratch.csv("passwords.csv", CHROME_CSV);
    import(&state, csv.clone());
    // A later export saved under the same name replaces the imported file.
    let moved = scratch.0.join("held.csv");
    fs::rename(&csv, &moved).unwrap();
    let newer = scratch.csv("newer.csv", NEWER_CSV);
    fs::rename(&newer, &csv).unwrap();
    assert!(matches!(
        state.delete_imported_file(),
        Err(VaultError::ImportedFileChanged)
    ));
    assert_eq!(fs::read_to_string(&csv).unwrap(), NEWER_CSV);
    // The imported file itself, moved back under its name, is still deleted.
    fs::rename(&moved, &csv).unwrap();
    state.delete_imported_file().unwrap();
    assert!(!csv.exists());
    assert_eq!(ready(state.overview("").unwrap()).total, 2);
}

#[test]
fn a_csv_rewritten_in_place_after_import_is_not_deleted() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let csv = scratch.csv("passwords.csv", CHROME_CSV);
    import(&state, csv.clone());
    // The same file, truncated and written again with other passwords.
    fs::write(&csv, NEWER_CSV).unwrap();
    assert!(matches!(
        state.delete_imported_file(),
        Err(VaultError::ImportedFileChanged)
    ));
    assert_eq!(fs::read_to_string(&csv).unwrap(), NEWER_CSV);
}

#[test]
fn only_the_last_imported_csv_can_be_deleted() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    assert!(matches!(
        state.delete_imported_file(),
        Err(VaultError::Invalid(_))
    ));
    let first = scratch.csv("first.csv", CHROME_CSV);
    let second = scratch.csv("second.csv", CHROME_CSV);
    import(&state, first.clone());
    import(&state, second.clone());
    state.delete_imported_file().unwrap();
    assert!(first.exists());
    assert!(!second.exists());
    assert!(matches!(
        state.delete_imported_file(),
        Err(VaultError::Invalid(_))
    ));
}

#[test]
fn edits_are_bounded_before_the_vault_opens() {
    let scratch = Scratch::new();
    let state = state(&scratch.0, 1);
    let id = Uuid::new_v4().to_string();
    let long_username = "u".repeat(super::MAX_USERNAME_BYTES + 1);
    assert!(matches!(
        state.update(&id, &long_username, None),
        Err(VaultError::Invalid(_))
    ));
    let empty = Secret::new(String::new());
    assert!(matches!(
        state.update(&id, "ada", Some(&empty)),
        Err(VaultError::Invalid(_))
    ));
    let long = Secret::new("p".repeat(super::MAX_PASSWORD_BYTES + 1));
    assert!(matches!(
        state.update(&id, "ada", Some(&long)),
        Err(VaultError::Invalid(_))
    ));
    assert!(matches!(
        state.overview(&"q".repeat(257)),
        Err(VaultError::Invalid(_))
    ));
    assert!(!scratch.0.join("passwords.db").exists());
}

#[test]
fn reading_from_an_empty_vault_creates_no_key() {
    let scratch = Scratch::new();
    let state = state(&scratch.0, 1);
    let id = Uuid::new_v4().to_string();
    assert!(matches!(state.password(&id), Err(VaultError::NotFound)));
    assert!(matches!(
        state.check_readable(&id),
        Err(VaultError::NotFound)
    ));
    assert!(matches!(
        state.update(&id, "ada", None),
        Err(VaultError::NotFound)
    ));
    assert!(!scratch.0.join("passwords.key").exists());
}

#[test]
fn a_cancelled_merge_saves_nothing() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        store.merge(
            &key,
            Source::Csv,
            &[credential("https://example.com", "ada", "one")],
            &cancelled
        ),
        Err(VaultError::Cancelled)
    ));
    assert_eq!(store.page("").unwrap().total, 0);
}

/// Cancels once `remaining` more rows have been checked.
struct CancelAfter(AtomicUsize);

impl Cancellation for CancelAfter {
    fn cancelled(&self) -> bool {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(1)
            })
            .is_err()
    }
}

#[test]
fn a_merge_cancelled_partway_rolls_back_every_row_it_wrote() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    merge(
        &mut store,
        &key,
        &[credential("https://a.example", "ada", "old")],
    );
    let ada = store.page("").unwrap().entries[0].id.clone();
    let rows = [
        credential("https://a.example", "ada", "changed"),
        credential("https://b.example", "grace", "new"),
        credential("https://c.example", "alan", "new"),
    ];
    assert!(matches!(
        store.merge(&key, Source::Csv, &rows, &CancelAfter(AtomicUsize::new(2))),
        Err(VaultError::Cancelled)
    ));
    let page = store.page("").unwrap();
    assert_eq!((page.total, page.entries.len()), (1, 1));
    assert_eq!(store.password(&key, &ada).unwrap().expose(), "old");
}

#[test]
fn cancellation_after_the_last_row_still_rolls_back_the_import() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    merge(
        &mut store,
        &key,
        &[credential("https://example.com", "ada", "old")],
    );
    let id = store.page("").unwrap().entries[0].id.clone();
    assert!(matches!(
        store.merge(
            &key,
            Source::Csv,
            &[credential("https://example.com", "ada", "new")],
            &CancelAfter(AtomicUsize::new(1))
        ),
        Err(VaultError::Cancelled)
    ));
    assert_eq!(store.page("").unwrap().total, 1);
    assert_eq!(store.password(&key, &id).unwrap().expose(), "old");
}

#[test]
fn a_cancel_sent_while_the_file_is_being_chosen_stops_that_import() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let csv = scratch.csv("passwords.csv", CHROME_CSV);
    let run = state.begin_import().unwrap();
    assert!(state.cancel_import());
    assert!(run.cancelled());
    assert!(matches!(
        run.import(csv.clone()),
        Err(VaultError::Cancelled)
    ));
    drop(run);
    assert_eq!(ready(state.overview("").unwrap()).total, 0);
    // The cancel belonged to that import alone.
    assert_eq!(import(&state, csv).added, 2);
}

#[test]
fn a_queued_cancellation_cannot_reach_a_later_import() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let first = state.begin_import().unwrap();
    let cancel = state.cancel_import_request();
    drop(first);
    let later = state.begin_import().unwrap();
    assert!(cancel());
    later
        .import(scratch.csv("passwords.csv", CHROME_CSV))
        .unwrap();
    assert_eq!(ready(state.overview("").unwrap()).total, 2);
}

#[test]
fn concurrent_import_requests_admit_exactly_one() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let start = Barrier::new(8);
    let runs: Vec<_> = thread::scope(|scope| {
        let requests: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    state.begin_import()
                })
            })
            .collect();
        requests
            .into_iter()
            .map(|request| request.join().unwrap())
            .collect()
    });
    let admitted = runs.iter().filter(|run| run.is_ok()).count();
    assert_eq!(admitted, 1);
    assert!(
        runs.iter()
            .all(|run| run.is_ok() || matches!(run, Err(VaultError::Invalid(_))))
    );
    drop(runs);
    state.cancel_import();
    assert!(!state.begin_import().unwrap().cancelled());
}

/// Pauses the actual SQLite commit after ImportRun admits it, allowing a
/// Cancel request to overlap the durable write without relying on timing.
struct PausedCommit<'a> {
    run: &'a super::state::ImportRun,
    entered: &'a Barrier,
    release: &'a Barrier,
}

impl Cancellation for PausedCommit<'_> {
    fn cancelled(&self) -> bool {
        self.run.cancelled()
    }

    fn commit(&self, write: impl FnOnce() -> VaultResult<()>) -> VaultResult<()> {
        self.run.commit(|| {
            self.entered.wait();
            self.release.wait();
            write()
        })
    }
}

#[test]
fn cancellation_during_commit_reports_too_late_and_keeps_the_saved_password() {
    let scratch = Scratch::new();
    let state = state(&scratch.0.join("profile"), 1);
    let run = state.begin_import().unwrap();
    let entered = Barrier::new(2);
    let release = Barrier::new(2);
    thread::scope(|scope| {
        let importing = scope.spawn(|| {
            let mut store = Store::in_memory().unwrap();
            let key = VaultKey::generate().unwrap();
            let counts = store
                .merge(
                    &key,
                    Source::Csv,
                    &[credential("https://example.com", "ada", "saved")],
                    &PausedCommit {
                        run: &run,
                        entered: &entered,
                        release: &release,
                    },
                )
                .unwrap();
            (store, key, counts)
        });
        entered.wait();
        // Commit is paused inside the admitted durable write. Cancel must
        // reject now, before the test permits SQLite to finish that write.
        let accepted = state.cancel_import();
        release.wait();
        let (store, key, counts) = importing.join().unwrap();
        assert_eq!(counts.added, 1);
        let page = store.page("").unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(
            store.password(&key, &page.entries[0].id).unwrap().expose(),
            "saved"
        );
        assert!(!accepted, "a durable commit cannot acknowledge rollback");
        // Even before the import reply is delivered, Cancel must say too late.
        assert!(!state.cancel_import());
    });
}

#[test]
fn an_import_past_the_vault_cap_is_refused_whole() {
    let mut store = Store::in_memory().unwrap();
    let key = VaultKey::generate().unwrap();
    let full: Vec<_> = (0..super::store::MAX_STORED)
        .map(|index| credential("https://a.example", &format!("user{index}"), "pw"))
        .collect();
    merge(&mut store, &key, &full);
    assert!(matches!(
        store.merge(
            &key,
            Source::Csv,
            &[
                credential("https://a.example", "user0", "changed"),
                credential("https://b.example", "one-more", "pw"),
            ],
            &AtomicBool::new(false)
        ),
        Err(VaultError::Import(_))
    ));
    let page = store.page("").unwrap();
    assert_eq!(page.total, super::store::MAX_STORED);
    let first = store.page("user0").unwrap().entries;
    let user0 = first
        .iter()
        .find(|entry| entry.username == "user0")
        .unwrap();
    assert_eq!(store.password(&key, &user0.id).unwrap().expose(), "pw");
}
