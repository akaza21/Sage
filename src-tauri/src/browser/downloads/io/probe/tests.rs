use super::*;
use crate::browser::downloads::folder::{admit_folder, admit_folder_before};

fn scratch() -> PathBuf {
    let folder = std::env::temp_dir().join(format!("sage-folder-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&folder).unwrap();
    folder
}

async fn drained(executor: &Executor) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while executor.workers.load(Ordering::Acquire) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("destination workers must stop after their OS calls return");
}

#[tokio::test]
async fn duplicate_probes_share_the_same_admitted_folder() {
    let executor = Arc::new(Executor::new(Duration::from_secs(2)));
    let folder = scratch();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = tokio::sync::oneshot::channel();
    let io = Arc::clone(&executor);
    let path = folder.clone();
    let first = tokio::spawn(async move {
        io.probe(path, move |path, deadline| {
            started.send(()).ok();
            let _ = wait.recv_timeout(Duration::from_secs(30));
            admit_folder_before(path, deadline)
        })
        .await
    });
    start.await.unwrap();
    let second = executor.probe(folder.clone(), |_, _| None);
    let (_, second) = tokio::join!(
        async {
            tokio::task::yield_now().await;
            drop(release);
        },
        second
    );
    let expected = admit_folder(&folder);
    assert!(expected.is_some());
    assert_eq!(first.await.unwrap(), expected);
    assert_eq!(second, expected);
    drained(&executor).await;
    std::fs::remove_dir_all(folder).unwrap();
}

async fn independent_probes(first_path: PathBuf, second_path: PathBuf) {
    let executor = Arc::new(Executor::new(Duration::from_secs(2)));
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = tokio::sync::oneshot::channel();
    let io = Arc::clone(&executor);
    let expected = first_path.clone();
    let first = tokio::spawn(async move {
        io.probe(first_path, move |path, _| {
            started.send(()).ok();
            let _ = wait.recv_timeout(Duration::from_secs(5));
            Some(path.to_owned())
        })
        .await
    });
    start.await.unwrap();
    let second = executor
        .probe(second_path.clone(), |path, _| Some(path.to_owned()))
        .await;
    drop(release);
    let first = first.await.unwrap();
    drained(&executor).await;
    assert_eq!(second, Some(second_path));
    assert_eq!(first, Some(expected));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn distinct_case_directories_do_not_share_a_pending_probe() {
    let folder = scratch();
    let upper = folder.join("A");
    let lower = folder.join("a");
    std::fs::create_dir(&upper).unwrap();
    std::fs::create_dir(&lower).unwrap();
    independent_probes(
        std::fs::canonicalize(upper).unwrap(),
        std::fs::canonicalize(lower).unwrap(),
    )
    .await;
    std::fs::remove_dir_all(folder).unwrap();
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn non_unicode_paths_do_not_share_a_pending_probe() {
    #[cfg(windows)]
    let names = {
        use std::os::windows::ffi::OsStringExt;
        [0xd800, 0xd801].map(|unit| std::ffi::OsString::from_wide(&[unit]))
    };
    #[cfg(unix)]
    let names = {
        use std::os::unix::ffi::OsStringExt;
        [0xfe, 0xff].map(|byte| std::ffi::OsString::from_vec(vec![byte]))
    };
    // Lanes are keyed by the supplied OS spelling without touching the
    // filesystem, so the folders need not exist. macOS volumes cannot hold
    // them: APFS rejects names that are not UTF-8.
    let [first, second] = names.map(|name| std::env::temp_dir().join(name));
    independent_probes(first, second).await;
}

#[tokio::test]
async fn deadline_releases_admission_before_the_os_call_returns() {
    let executor = Arc::new(Executor::new(Duration::from_secs(1)));
    let folder = scratch();
    let held = (0..MAX_LEASES - 1)
        .map(|index| {
            executor
                .acquire(&folder.join(format!("download-{}", index / MAX_PER_DESTINATION)))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = tokio::sync::oneshot::channel();
    let io = Arc::clone(&executor);
    let remote = folder.join("remote");
    let probe = tokio::spawn(async move {
        io.probe(remote, move |path, _| {
            started.send(()).ok();
            let _ = wait.recv_timeout(Duration::from_secs(30));
            Some(path.to_owned())
        })
        .await
    });
    start.await.unwrap();
    assert!(
        executor.acquire(&folder).is_none(),
        "total admission must be bounded"
    );
    assert_eq!(probe.await.unwrap(), None);
    let (done, completion) = tokio::sync::oneshot::channel();
    let output = folder.join("local.txt");
    let path = output.clone();
    assert!(executor.acquire(&folder).unwrap().run(
        move |_, deadline| {
            deadline.check()?;
            std::fs::write(path, b"local download")
        },
        move |result| {
            done.send(result).ok();
        },
        |_| {}
    ));
    completion.await.unwrap().unwrap();
    assert_eq!(std::fs::read(output).unwrap(), b"local download");
    drop(held);
    drop(release);
    drained(&executor).await;
    std::fs::remove_dir_all(folder).unwrap();
}

#[tokio::test]
async fn reset_probes_cannot_grow_the_number_of_blocked_threads() {
    let executor = Arc::new(Executor::new(Duration::from_secs(2)));
    let folder = scratch();
    let mut releases = Vec::new();
    for index in 0..MAX_LEASES {
        let (release, wait) = mpsc::sync_channel::<()>(0);
        let (started, start) = tokio::sync::oneshot::channel();
        let io = Arc::clone(&executor);
        let path = folder.join(index.to_string());
        let probe = tokio::spawn(async move {
            io.probe(path, move |path, _| {
                started.send(()).ok();
                let _ = wait.recv_timeout(Duration::from_secs(30));
                Some(path.to_owned())
            })
            .await
        });
        start.await.unwrap();
        executor.cancel_probes();
        assert_eq!(probe.await.unwrap(), None);
        releases.push(release);
    }
    for index in 0..100 {
        assert_eq!(
            executor
                .probe(folder.join((index % MAX_LEASES).to_string()), |path, _| {
                    Some(path.to_owned())
                })
                .await,
            None
        );
        assert_eq!(
            executor
                .probe(folder.join(format!("new-{index}")), |path, _| {
                    Some(path.to_owned())
                })
                .await,
            None
        );
    }
    assert_eq!(executor.active.load(Ordering::Acquire), 0);
    assert_eq!(executor.workers.load(Ordering::Acquire), MAX_LEASES);
    drop(releases);
    drained(&executor).await;
    assert_eq!(
        executor.probe(folder.clone(), admit_folder_before).await,
        admit_folder(&folder)
    );
    drained(&executor).await;
    std::fs::remove_dir_all(folder).unwrap();
}
