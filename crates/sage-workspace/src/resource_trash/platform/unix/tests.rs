use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use uuid::Uuid;

use super::*;

thread_local! {
    static HOME: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// The data directory a test on this thread trashes into, instead of the
/// user's own.
#[cfg(target_os = "linux")]
pub(super) fn home_override() -> Option<PathBuf> {
    HOME.with(|home| home.borrow().clone())
}

struct Fixture {
    base: PathBuf,
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!("sage-unix-trash-{}", Uuid::new_v4()));
        fs::create_dir_all(base.join("project/src/empty")).unwrap();
        fs::create_dir_all(base.join("home")).unwrap();
        fs::write(base.join("project/src/file.rs"), "preserve\n").unwrap();
        let base = base.canonicalize().unwrap();
        HOME.with(|home| *home.borrow_mut() = Some(base.join("home")));
        Self {
            root: base.join("project"),
            home: base.join("home"),
            base,
        }
    }

    fn parent(&self, relative_path: &'static str) -> ParentAuthority<'_> {
        ParentAuthority {
            root: &self.root,
            expected_root: root_identity_path(&self.root).unwrap(),
            relative_path,
        }
    }

    fn recycle(&self, name: &str, kind: TrashResourceKind) -> (AppResult<TrashObservation>, bool) {
        let expected = observe_identity(self.parent("src"), name, kind).unwrap();
        self.recycle_expecting(name, kind, expected)
    }

    fn recycle_expecting(
        &self,
        name: &str,
        kind: TrashResourceKind,
        expected: ResourceTrashIdentity,
    ) -> (AppResult<TrashObservation>, bool) {
        let started = Arc::new(AtomicBool::new(false));
        let marker = Arc::clone(&started);
        let result = recycle_one(
            self.parent("src"),
            name,
            kind,
            expected,
            move |_| marker.store(true, Ordering::SeqCst),
            || Ok(()),
            || Err(AppError::InvalidRequest("listing is not under test".into())),
        );
        (result, started.load(Ordering::SeqCst))
    }

    fn trash(&self) -> PathBuf {
        self.home.join("Trash")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        HOME.with(|home| *home.borrow_mut() = None);
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn trash_entries(fixture: &Fixture) -> usize {
    fs::read_dir(fixture.trash().join("files")).map_or(0, Iterator::count)
}

#[cfg(target_os = "linux")]
#[test]
fn a_file_goes_to_the_home_trash_with_a_record_that_restores_it() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    fs::write(fixture.root.join("src/a b%.rs"), "preserve\n").unwrap();

    let (result, started) = fixture.recycle("a b%.rs", TrashResourceKind::File);
    let observation = result.unwrap();

    assert!(started);
    assert!(observation.warning.is_none());
    assert!(observation.source_absent_confirmed);
    assert!(!fixture.root.join("src/a b%.rs").exists());
    assert_eq!(
        fs::read_to_string(fixture.trash().join("files/a b%.rs")).unwrap(),
        "preserve\n"
    );
    let info = fs::read_to_string(fixture.trash().join("info/a b%.rs.trashinfo")).unwrap();
    let mut lines = info.lines();
    assert_eq!(lines.next(), Some("[Trash Info]"));
    assert_eq!(
        lines.next().unwrap(),
        format!("Path={}/src/a%20b%25.rs", fixture.root.display())
    );
    let date = lines.next().unwrap().strip_prefix("DeletionDate=").unwrap();
    assert!(
        date.len() == 19 && date.as_bytes()[10] == b'T',
        "unexpected date {date}"
    );
    assert_eq!(lines.next(), None);
    let mode = fs::metadata(fixture.trash().join("files"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700);
}

#[cfg(target_os = "linux")]
#[test]
fn an_empty_folder_goes_to_the_trash() {
    let fixture = Fixture::new();

    let (result, started) = fixture.recycle("empty", TrashResourceKind::EmptyFolder);

    assert!(result.unwrap().source_absent_confirmed);
    assert!(started);
    assert!(fixture.trash().join("files/empty").is_dir());
    assert!(fixture.trash().join("info/empty.trashinfo").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn taken_names_get_a_numbered_name_and_older_items_are_kept() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.trash().join("files")).unwrap();
    fs::create_dir_all(fixture.trash().join("info")).unwrap();
    // An earlier item's record, and an item whose record is missing.
    fs::write(fixture.trash().join("info/file.rs.trashinfo"), "older\n").unwrap();
    fs::write(fixture.trash().join("files/file.2.rs"), "orphan\n").unwrap();

    let (result, _) = fixture.recycle("file.rs", TrashResourceKind::File);

    assert!(result.unwrap().warning.is_none());
    assert_eq!(
        fs::read_to_string(fixture.trash().join("files/file.3.rs")).unwrap(),
        "preserve\n"
    );
    assert!(fixture.trash().join("info/file.3.rs.trashinfo").is_file());
    assert_eq!(
        fs::read_to_string(fixture.trash().join("info/file.rs.trashinfo")).unwrap(),
        "older\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.trash().join("files/file.2.rs")).unwrap(),
        "orphan\n"
    );
    assert!(!fixture.trash().join("info/file.2.rs.trashinfo").exists());
}

#[test]
fn links_and_kind_changes_are_refused_before_anything_moves() {
    let fixture = Fixture::new();
    let src = fixture.root.join("src");
    fs::write(src.join("linked.rs"), "preserve\n").unwrap();
    fs::hard_link(src.join("linked.rs"), src.join("alias.rs")).unwrap();
    std::os::unix::fs::symlink(src.join("file.rs"), src.join("link.rs")).unwrap();

    for (name, kind, expected) in [
        ("linked.rs", TrashResourceKind::File, "hard-link aliases"),
        ("link.rs", TrashResourceKind::File, "symbolic link"),
        ("file.rs", TrashResourceKind::EmptyFolder, "kind changed"),
        ("empty", TrashResourceKind::File, "kind changed"),
    ] {
        let error = observe_identity(fixture.parent("src"), name, kind).unwrap_err();
        assert!(error.to_string().contains(expected), "{name}: {error}");
    }
    assert!(src.join("linked.rs").is_file());
    assert!(src.join("link.rs").is_symlink());
    assert_eq!(trash_entries(&fixture), 0);
}

#[test]
fn a_source_replaced_after_admission_is_refused_without_mutation() {
    let fixture = Fixture::new();
    let expected =
        observe_identity(fixture.parent("src"), "file.rs", TrashResourceKind::File).unwrap();
    let source = fixture.root.join("src/file.rs");
    fs::rename(&source, fixture.root.join("src/original.rs")).unwrap();
    fs::write(&source, "replacement\n").unwrap();

    let (result, started) = fixture.recycle_expecting("file.rs", TrashResourceKind::File, expected);

    let Err(error) = result else {
        panic!("a replaced source must be refused");
    };
    assert!(
        error
            .to_string()
            .contains("changed after durable intent admission")
    );
    assert!(!started);
    assert_eq!(fs::read_to_string(&source).unwrap(), "replacement\n");
    assert_eq!(trash_entries(&fixture), 0);
}

#[test]
fn a_parent_reached_through_a_link_is_refused() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink(fixture.root.join("src"), fixture.root.join("linked")).unwrap();

    let error =
        observe_identity(fixture.parent("linked"), "file.rs", TrashResourceKind::File).unwrap_err();

    assert!(matches!(error, AppError::Io(_)), "{error}");
    assert!(fixture.root.join("src/file.rs").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn an_item_swapped_in_at_the_source_name_is_put_back() {
    let fixture = Fixture::new();
    let authority =
        open_authority(fixture.parent("src"), "file.rs", TrashResourceKind::File).unwrap();
    fs::create_dir(fixture.trash()).unwrap();
    let trash = freedesktop::trash_directory(&fs::File::open(fixture.trash()).unwrap()).unwrap();
    // The held source moved away and a different file was trashed instead.
    fs::rename(
        fixture.root.join("src/file.rs"),
        fixture.root.join("src/moved.rs"),
    )
    .unwrap();
    fs::write(fixture.trash().join("files/file.rs"), "swapped\n").unwrap();
    fs::write(fixture.trash().join("info/file.rs.trashinfo"), "record\n").unwrap();

    let warning = freedesktop::settle(&authority, &trash, c"file.rs", c"file.rs.trashinfo");

    assert!(warning.unwrap().contains("put that item back"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/file.rs")).unwrap(),
        "swapped\n"
    );
    assert!(!fixture.trash().join("files/file.rs").exists());
    assert!(!fixture.trash().join("info/file.rs.trashinfo").exists());
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/moved.rs")).unwrap(),
        "preserve\n"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_item_that_cannot_be_put_back_stays_in_the_trash_with_a_warning() {
    let fixture = Fixture::new();
    let authority =
        open_authority(fixture.parent("src"), "file.rs", TrashResourceKind::File).unwrap();
    fs::create_dir(fixture.trash()).unwrap();
    let trash = freedesktop::trash_directory(&fs::File::open(fixture.trash()).unwrap()).unwrap();
    // The source name is occupied again, so putting back would replace it.
    fs::write(fixture.trash().join("files/file.rs"), "swapped\n").unwrap();

    let warning = freedesktop::settle(&authority, &trash, c"file.rs", c"file.rs.trashinfo");

    assert!(warning.unwrap().contains("could not be put back"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("src/file.rs")).unwrap(),
        "preserve\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.trash().join("files/file.rs")).unwrap(),
        "swapped\n"
    );
}

#[cfg(target_os = "linux")]
fn uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(target_os = "linux")]
#[test]
fn another_volume_uses_its_own_trash_directory_per_the_specification() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let top = fixture.base.join("top");
    let volume = |path: &std::path::Path| fs::metadata(path).unwrap().dev();
    let open = |path: &std::path::Path| fs::File::open(path).unwrap();

    // No shared directory: a private .Trash-$uid is made.
    fs::create_dir(&top).unwrap();
    assert!(freedesktop::topdir_trash(&open(&top), volume(&top)).is_some());
    let private = top.join(format!(".Trash-{}", uid()));
    assert!(private.join("files").is_dir() && private.join("info").is_dir());
    assert_eq!(fs::metadata(&private).unwrap().mode() & 0o777, 0o700);
    fs::remove_dir_all(&private).unwrap();

    // A sticky shared directory gets a per-user subdirectory.
    fs::create_dir(top.join(".Trash")).unwrap();
    fs::set_permissions(top.join(".Trash"), fs::Permissions::from_mode(0o1777)).unwrap();
    assert!(freedesktop::topdir_trash(&open(&top), volume(&top)).is_some());
    assert!(top.join(format!(".Trash/{}/files", uid())).is_dir());
    assert!(!private.exists());
    fs::remove_dir_all(top.join(".Trash")).unwrap();

    // A shared directory that is not sticky is ignored.
    fs::create_dir(top.join(".Trash")).unwrap();
    fs::set_permissions(top.join(".Trash"), fs::Permissions::from_mode(0o777)).unwrap();
    assert!(freedesktop::topdir_trash(&open(&top), volume(&top)).is_some());
    assert!(!top.join(format!(".Trash/{}", uid())).exists());
    assert!(private.join("files").is_dir());
    fs::remove_dir_all(&private).unwrap();
    fs::remove_dir_all(top.join(".Trash")).unwrap();

    // A private directory that is a link is refused, and its target untouched.
    let elsewhere = fixture.base.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &private).unwrap();
    assert!(freedesktop::topdir_trash(&open(&top), volume(&top)).is_none());
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn the_volume_top_is_the_highest_ancestor_and_records_hold_the_path_below_it() {
    use std::os::unix::ffi::OsStrExt;

    let fixture = Fixture::new();
    let device = fs::metadata(&fixture.root).unwrap().dev();
    let expected = fixture
        .root
        .ancestors()
        .take_while(|path| fs::metadata(path).unwrap().dev() == device)
        .last()
        .unwrap();
    let parent = fs::File::open(fixture.root.join("src")).unwrap();

    let (top, steps) = freedesktop::volume_top(&parent, device).unwrap();

    let (held, visible) = (top.metadata().unwrap(), fs::metadata(expected).unwrap());
    assert_eq!((held.dev(), held.ino()), (visible.dev(), visible.ino()));
    let source = fixture.root.join("src/file.rs");
    let below = freedesktop::path_below_top(&source, &top, steps).unwrap();
    assert_eq!(
        below,
        source
            .strip_prefix(expected)
            .unwrap()
            .as_os_str()
            .as_bytes()
    );
    assert!(!below.starts_with(b"/"));
    // A path that does not lead to the walked top is refused.
    assert!(freedesktop::path_below_top(&source, &top, steps + 1).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn records_escape_paths_as_glib_does() {
    assert_eq!(
        freedesktop::trash_info(
            "dir/a b%é!$&'()*+,;=:@#?.rs".as_bytes(),
            "2026-10-26T12:00:00"
        ),
        "[Trash Info]\nPath=dir/a%20b%25%C3%A9%21%24%26%27%28%29%2A%2B%2C%3B%3D%3A%40%23%3F.rs\nDeletionDate=2026-10-26T12:00:00\n"
    );
    assert!(
        freedesktop::trash_info(b"/home/u/x-y_z~.txt", "d").contains("Path=/home/u/x-y_z~.txt\n")
    );
}

#[test]
fn a_folder_filled_after_admission_is_refused_without_mutation() {
    let fixture = Fixture::new();
    let expected = observe_identity(
        fixture.parent("src"),
        "empty",
        TrashResourceKind::EmptyFolder,
    )
    .unwrap();
    fs::write(fixture.root.join("src/empty/new.rs"), "new\n").unwrap();

    let (result, started) =
        fixture.recycle_expecting("empty", TrashResourceKind::EmptyFolder, expected);

    let Err(error) = result else {
        panic!("a filled folder must be refused");
    };
    assert!(error.to_string().contains("only empty folders"), "{error}");
    assert!(!started);
    assert!(fixture.root.join("src/empty/new.rs").is_file());
    assert_eq!(trash_entries(&fixture), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn numbered_names_keep_the_extension_as_glib_does() {
    assert_eq!(freedesktop::numbered("notes.txt", 1), "notes.txt");
    assert_eq!(freedesktop::numbered("notes.txt", 2), "notes.2.txt");
    assert_eq!(freedesktop::numbered("a.tar.gz", 3), "a.tar.3.gz");
    assert_eq!(freedesktop::numbered(".env", 2), ".env.2");
    assert_eq!(freedesktop::numbered("Makefile", 2), "Makefile.2");
}

#[cfg(target_os = "linux")]
#[test]
fn the_workspace_moves_a_file_to_the_trash_and_confirms_it_from_the_listing() {
    let fixture = Fixture::new();
    let workspace = crate::Workspace::open("project", "root", &fixture.root).unwrap();
    let source = workspace.resource("src/file.rs").unwrap();
    let identity = workspace
        .resource_trash_identity(&source, TrashResourceKind::File)
        .unwrap();
    let marker = Arc::new(crate::resource_trash::ResourceTrashMarker::default());

    let commit = workspace
        .trash_resource_observed(
            &source,
            TrashResourceKind::File,
            identity,
            Arc::clone(&marker),
        )
        .unwrap();

    assert!(commit.resource_confirmed, "{:?}", commit.warning);
    assert!(commit.warning.is_none());
    assert!(marker.mutation_started());
    assert_eq!(commit.parent.relative_path, "src");
    assert!(
        !commit
            .parent_listing
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.name == "file.rs")
    );
    assert!(fixture.trash().join("files/file.rs").is_file());
}

/// Runs on a Mac only, and leaves one small `sage-trash-test-….txt` in the
/// user's Trash, as the Windows tests leave items in the Recycle Bin.
#[cfg(target_os = "macos")]
#[test]
fn a_file_goes_to_the_system_trash_and_arrives_as_the_held_source() {
    let fixture = Fixture::new();
    let name = format!("sage-trash-test-{}.txt", Uuid::new_v4());
    fs::write(fixture.root.join("src").join(&name), "preserve\n").unwrap();

    let (result, started) = fixture.recycle(&name, TrashResourceKind::File);
    let observation = result.unwrap();

    assert!(started);
    assert!(observation.warning.is_none(), "{:?}", observation.warning);
    assert!(observation.source_absent_confirmed);
    assert!(!fixture.root.join("src").join(&name).exists());
}
