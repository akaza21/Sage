use super::*;
use crate::{
    browser::downloads::{DownloadRegistry, NavigationProvenance},
    settings_commands::browser_download_folder::{remember_first_choice, remember_folder},
    settings_store::SettingsStore,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sage-download-folder-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(admit_folder(&path).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn silent_path(plan: &Plan) -> Option<PathBuf> {
    match plan {
        Plan::Silent(reserved) => Some(reserved.path.clone()),
        Plan::Ask { .. } => None,
    }
}

fn free(_: &Path) -> bool {
    false
}

#[test]
fn slow_reservation_conflicts_stop_at_the_deadline_and_release_admission() {
    use crate::browser::downloads::{MAX_ACTIVE_DOWNLOADS, io::Executor};
    use std::{
        io,
        sync::mpsc,
        time::{Duration, Instant},
    };

    let folder = Scratch::new("slow-conflict");
    let executor = Executor::new(Duration::from_secs(2));
    let held = (0..MAX_ACTIVE_DOWNLOADS - 1)
        .map(|_| executor.acquire(&folder.0).unwrap())
        .collect::<Vec<_>>();
    let destination = executor.acquire(&folder.0).unwrap();
    assert!(executor.acquire(&folder.0).is_none());
    let path = folder.0.clone();
    let (finished, work_done) = mpsc::sync_channel(1);
    let (done, completion) = mpsc::sync_channel(1);
    assert!(destination.run(
        move |_, deadline| {
            let mut first = true;
            let reserved =
                unique_destination_with(&path, "report.txt", free, deadline, |candidate| {
                    if first {
                        first = false;
                        // Model one uninterruptible create_new on a slow share.
                        while deadline.check().is_ok() {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(io::ErrorKind::AlreadyExists.into())
                    } else {
                        placeholder::create(candidate)
                    }
                });
            let selected = reserved.as_ref().map(|value| value.path.clone());
            if let Some(reserved) = reserved {
                reserved.delete_if_empty();
            }
            finished.send(selected).unwrap();
            deadline.check()
        },
        move |result| {
            done.send(result).ok();
        },
        |_| {},
    ));
    assert_eq!(
        completion
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut,
    );
    let selected = work_done.recv_timeout(Duration::from_secs(5)).unwrap();
    // Download admission must become reusable when the expired OS call
    // returns, without starting another reservation or retaining the lease.
    let limit = Instant::now() + Duration::from_secs(2);
    let next = loop {
        if let Some(next) = executor.acquire(&folder.0) {
            break next;
        }
        assert!(
            Instant::now() < limit,
            "expired planning retained admission"
        );
        std::thread::yield_now();
    };
    drop((next, held));
    assert_eq!(selected, None, "expiry must stop the reservation search");
    assert!(!folder.0.join("report (1).txt").exists());
}

/// One download through the same steps the WebView2 handler takes: plan
/// against the stored folder, record the destination in the registry, and
/// remember the folder when the plan asked to.
async fn download(
    store: &SettingsStore,
    registry: &mut DownloadRegistry,
    suggested: &str,
    dialog_choice: Option<&Path>,
) -> (Plan, Option<PathBuf>) {
    let id = registry
        .begin("browser-one", "https://example.com/file", suggested)
        .unwrap();
    let stored = store.browser_download_folder_record().unwrap().path;
    let claimed = registry.claimed();
    let plan = plan(stored.as_deref(), suggested, None, |path| {
        claimed.contains(path)
    });
    let destination = match &plan {
        Plan::Silent(reserved) => Some(reserved.path.clone()),
        Plan::Ask { file_name, .. } => dialog_choice.map(|folder| folder.join(file_name)),
    };
    if let Some(path) = &destination {
        assert!(registry.record_destination(id, path.clone()));
        assert!(registry.destination_chosen(id));
        if matches!(plan, Plan::Ask { remember: true, .. }) {
            let folder = folder_of_choice(path).unwrap();
            remember_folder(store, &folder).await.unwrap();
        }
    }
    (plan, destination)
}

fn user_registry() -> DownloadRegistry {
    let mut registry = DownloadRegistry::default();
    registry.set_provenance("browser-one", NavigationProvenance::User);
    registry
}

#[tokio::test]
async fn first_download_asks_then_later_downloads_save_silently_to_that_folder() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let chosen = Scratch::new("first-ask");

    let (first, destination) = download(&store, &mut registry, "report.pdf", Some(&chosen.0)).await;
    assert_eq!(
        first,
        Plan::Ask {
            file_name: "report.pdf".into(),
            remember: true,
            directory: None,
        }
    );
    assert_eq!(destination, Some(chosen.0.join("report.pdf")));
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        Some(chosen.0.clone())
    );

    let (second, _) = download(&store, &mut registry, "photo.png", None).await;
    assert_eq!(silent_path(&second), Some(chosen.0.join("photo.png")));
}

#[tokio::test]
async fn a_first_dangerous_download_remembers_the_explicit_choice() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let chosen = Scratch::new("first-dangerous");
    let (first, _) = download(&store, &mut registry, "setup.exe", Some(&chosen.0)).await;
    assert!(matches!(first, Plan::Ask { remember: true, .. }));
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        Some(chosen.0.clone())
    );
    let (second, _) = download(&store, &mut registry, "report.pdf", None).await;
    assert_eq!(silent_path(&second), Some(chosen.0.join("report.pdf")));
}

#[tokio::test]
async fn a_canceled_first_save_dialog_saves_no_folder() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let (plan, destination) = download(&store, &mut registry, "report.pdf", None).await;
    assert!(matches!(plan, Plan::Ask { remember: true, .. }));
    assert_eq!(destination, None);
    assert_eq!(store.browser_download_folder_record().unwrap().path, None);
}

#[tokio::test]
async fn a_silent_save_never_overwrites_an_existing_file_or_an_in_flight_download() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let folder = Scratch::new("suffix");
    remember_folder(&store, &folder.0).await.unwrap();
    std::fs::write(folder.0.join("data.csv"), b"keep me").unwrap();

    let (first, _) = download(&store, &mut registry, "data.csv", None).await;
    assert_eq!(silent_path(&first), Some(folder.0.join("data (1).csv")));
    // The first is still downloading; the next one takes the next number.
    let (second, _) = download(&store, &mut registry, "data.csv", None).await;
    assert_eq!(silent_path(&second), Some(folder.0.join("data (2).csv")));
    assert_eq!(
        std::fs::read(folder.0.join("data.csv")).unwrap(),
        b"keep me"
    );
}

#[tokio::test]
async fn a_missing_saved_folder_asks_again_and_remembers_the_new_choice() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let old = Scratch::new("missing-old");
    remember_folder(&store, &old.0).await.unwrap();
    std::fs::remove_dir_all(&old.0).unwrap();
    let new = Scratch::new("missing-new");

    let (plan, _) = download(&store, &mut registry, "notes.txt", Some(&new.0)).await;
    assert!(matches!(
        plan,
        Plan::Ask {
            remember: true,
            directory: None,
            ..
        }
    ));
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        Some(new.0.clone())
    );
}

#[test]
fn a_saved_path_that_became_a_file_asks_again() {
    let parent = Scratch::new("became-file");
    let path = parent.0.join("downloads");
    std::fs::write(&path, b"not a folder").unwrap();
    assert!(matches!(
        plan(Some(&path), "a.txt", None, free),
        Plan::Ask { remember: true, .. }
    ));
}

#[tokio::test]
async fn dangerous_types_always_ask_and_keep_the_saved_folder() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let saved = Scratch::new("dangerous-saved");
    let elsewhere = Scratch::new("dangerous-elsewhere");
    remember_folder(&store, &saved.0).await.unwrap();
    for name in [
        "setup.exe",
        "Installer.MSI",
        "run.bat",
        "script.ps1",
        "screen.scr",
        "shortcut.lnk",
        "invoice.pdf.exe",
        "trailing.exe. . ",
    ] {
        let (plan, _) = download(&store, &mut registry, name, Some(&elsewhere.0)).await;
        let Plan::Ask {
            remember,
            directory,
            ..
        } = plan
        else {
            panic!("{name} saved without asking");
        };
        assert!(!remember, "{name}");
        assert_eq!(directory.as_deref(), Some(saved.0.as_path()), "{name}");
    }
    assert_eq!(
        store.browser_download_folder_record().unwrap().path,
        Some(saved.0.clone())
    );
    // A stream colon cannot reach the file system, so the name is no
    // longer an executable.
    assert_eq!(
        silent_path(&plan(Some(&saved.0), "stream.exe:zone", None, free)),
        Some(saved.0.join("stream.exe_zone"))
    );
}

#[tokio::test]
async fn dangerous_downloads_keep_an_unavailable_remembered_folder() {
    let store = SettingsStore::in_memory().unwrap();
    let saved = Scratch::new("dangerous-unavailable");
    store.set_browser_download_folder(&saved.0).unwrap();
    std::fs::remove_dir_all(&saved.0).unwrap();
    let chosen = Scratch::new("dangerous-choice");

    for (name, mime) in [
        ("setup.exe", None),
        ("report.txt", Some("application/x-msdownload")),
    ] {
        let record = store.browser_download_folder_record().unwrap();
        let Plan::Ask {
            file_name,
            remember,
            directory,
        } = plan(record.path.as_deref(), name, mime, free)
        else {
            panic!("{name} saved without asking");
        };
        assert_eq!(directory, None);
        if remember {
            assert!(
                remember_first_choice(&store, record.revision, &chosen.0.join(file_name),).await
            );
        }
        assert_eq!(
            store.browser_download_folder_record().unwrap().path,
            Some(saved.0.clone()),
            "{name} changed the remembered folder",
        );
    }
}

#[test]
fn ordinary_types_are_not_dangerous() {
    for name in [
        "report.pdf",
        "archive.zip",
        "photo.jpeg",
        "README",
        ".env",
        "data.csv",
    ] {
        assert!(!is_dangerous(name), "{name}");
    }
    for name in [
        "a.EXE", "b.cmd", "c.vbs", "d.msix", "e.hta", "f.reg", "g.jar",
    ] {
        assert!(is_dangerous(name), "{name}");
    }
}

#[test]
fn file_names_lose_traversal_separators_and_reserved_forms() {
    let cases = [
        ("../../etc/passwd", "passwd"),
        (r"..\..\Windows\win.ini", "win.ini"),
        ("..", "download"),
        ("", "download"),
        ("   ", "download"),
        ("a:b*c?.txt", "a_b_c_.txt"),
        ("name.txt. . ", "name.txt"),
        ("CON.txt", "_CON.txt"),
        ("lpt1", "_lpt1"),
        ("COM\u{b9}.txt", "_COM\u{b9}.txt"),
        ("conout$", "_conout$"),
        ("nul .tar.gz", "_nul .tar.gz"),
        ("evil\u{202e}gpj.exe", "evilgpj.exe"),
        ("tab\tname\n.txt", "tabname.txt"),
        ("console.log", "console.log"),
    ];
    for (input, expected) in cases {
        assert_eq!(sanitize_file_name(input), expected, "{input:?}");
    }
}

#[test]
fn long_file_names_are_bounded_and_keep_their_extension() {
    let name = format!("{}.pdf", "é".repeat(300));
    let sanitized = sanitize_file_name(&name);
    assert!(sanitized.len() <= MAX_NAME_BYTES);
    assert!(sanitized.ends_with(".pdf"));
    let name = format!("a.{}", "x".repeat(400));
    assert!(sanitize_file_name(&name).len() <= MAX_NAME_BYTES);
}

#[test]
fn every_destination_is_a_direct_child_of_the_folder() {
    let folder = Scratch::new("child");
    for name in ["../escape.txt", r"..\escape.txt", "/abs.txt", "C:evil.txt"] {
        let Plan::Silent(Placeholder { path, .. }) = plan(Some(&folder.0), name, None, free) else {
            panic!("{name} asked");
        };
        assert_eq!(path.parent(), Some(folder.0.as_path()), "{name}");
    }
}

#[test]
fn a_relative_stored_folder_is_never_used() {
    assert!(matches!(
        plan(Some(Path::new("downloads")), "a.txt", None, free),
        Plan::Ask { remember: true, .. }
    ));
}

#[test]
fn verbatim_windows_paths_are_simplified_only_when_equivalent() {
    assert_eq!(
        simplified_verbatim_text(r"\\?\C:\downloads").as_deref(),
        Some(r"C:\downloads")
    );
    assert_eq!(
        simplified_verbatim_text(r"\\?\UNC\server\share\dl").as_deref(),
        Some(r"\\server\share\dl")
    );
    assert_eq!(simplified_verbatim_text(r"\\?\C:\odd.\dir"), None);
    assert_eq!(simplified_verbatim_text(r"\\?\Volume{x}\dir"), None);
    assert_eq!(simplified_verbatim_text("/home/me"), None);
    let long = format!(r"\\?\C:\{}", "a".repeat(300));
    assert_eq!(simplified_verbatim_text(&long), None);
}

#[cfg(unix)]
#[test]
fn a_read_only_saved_folder_asks_again() {
    use std::os::unix::fs::PermissionsExt;
    let folder = Scratch::new("read-only");
    std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root writes anywhere; the probe is still honest about that.
    let writable = is_writable(&folder.0);
    let planned = plan(Some(&folder.0), "a.txt", None, free);
    std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        assert!(matches!(planned, Plan::Silent(_)));
    } else {
        assert!(matches!(planned, Plan::Ask { remember: true, .. }));
    }
}

#[tokio::test]
async fn downloads_differing_only_in_case_never_share_a_destination() {
    let store = SettingsStore::in_memory().unwrap();
    let mut registry = user_registry();
    let folder = Scratch::new("case");
    remember_folder(&store, &folder.0).await.unwrap();
    let (first, _) = download(&store, &mut registry, "Report.pdf", None).await;
    assert_eq!(silent_path(&first), Some(folder.0.join("Report.pdf")));
    let (second, _) = download(&store, &mut registry, "report.pdf", None).await;
    assert_eq!(silent_path(&second), Some(folder.0.join("report (1).pdf")));
}

#[test]
fn a_silent_save_reserves_its_name_on_disk() {
    let folder = Scratch::new("reserve");
    let Plan::Silent(first) = plan(Some(&folder.0), "a.txt", None, free) else {
        panic!("asked");
    };
    assert_eq!(std::fs::metadata(&first.path).unwrap().len(), 0);
    // Without any registry claim, the reservation alone moves the next one on.
    let Plan::Silent(second) = plan(Some(&folder.0), "a.txt", None, free) else {
        panic!("asked");
    };
    assert_eq!(second.path, folder.0.join("a (1).txt"));
}

#[test]
fn a_dangerous_content_type_asks_whatever_the_name_says() {
    let folder = Scratch::new("mime");
    for mime in [
        "application/x-msdownload",
        "Application/X-MSDownload; charset=binary",
        "application/vnd.microsoft.portable-executable",
        "application/x-msi",
    ] {
        assert!(
            matches!(
                plan(Some(&folder.0), "report.pdf", Some(mime), free),
                Plan::Ask {
                    remember: false,
                    ..
                }
            ),
            "{mime}"
        );
    }
    assert!(matches!(
        plan(Some(&folder.0), "report.pdf", Some("application/pdf"), free),
        Plan::Silent(_)
    ));
}

#[test]
fn office_add_ins_and_other_installers_are_dangerous() {
    for name in [
        "addin.vsto",
        "x.ocx",
        "x.cab",
        "x.appinstaller",
        "x.msixbundle",
        "x.settingcontent-ms",
        "x.xlam",
        "x.wsf",
        "x.cpl",
        "x.application",
    ] {
        assert!(is_dangerous(name), "{name}");
    }
}

#[test]
fn the_write_probe_leaves_nothing_behind() {
    let folder = Scratch::new("probe");
    assert!(admit_folder(&folder.0).is_some());
    let left: Vec<_> = std::fs::read_dir(&folder.0).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn the_unix_write_check_creates_nothing_and_follows_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let folder = Scratch::new("access");
    assert!(is_writable(&folder.0));
    std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    let read_only = is_writable(&folder.0);
    std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY: geteuid has no preconditions.
    let root = unsafe { libc::geteuid() } == 0;
    assert_eq!(read_only, root, "only root may write to a 0o555 folder");
    assert!(!is_writable(&folder.0.join("missing")));
    let left: Vec<_> = std::fs::read_dir(&folder.0).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
}
