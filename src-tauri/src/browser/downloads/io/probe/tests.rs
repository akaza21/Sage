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
    // A second physical probe would refuse this folder. Both callers must
    // instead receive the successful result of the already-running check.
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

#[tokio::test]
async fn deadline_releases_admission_before_the_os_call_returns() {
    let executor = Arc::new(Executor::new(Duration::from_secs(1)));
    let folder = scratch();
    // Saturate shared admission with live download leases and one probe.
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
    // This needs the expired probe's slot, while its OS call is still blocked.
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
    // Expired markers must coalesce, and fresh destinations must fail closed
    // once physical workers are exhausted, even though admission is now free.
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
    // A late successful OS return cannot retain capacity or poison recovery.
    assert_eq!(
        executor.probe(folder.clone(), admit_folder_before).await,
        admit_folder(&folder)
    );
    drained(&executor).await;
    std::fs::remove_dir_all(folder).unwrap();
}
