use std::{path::Path, sync::mpsc, time::Duration};

use super::super::choice::Step;
use super::*;
use crate::{
    browser::downloads::{folder, worker},
    settings_store::DownloadFolderRecord,
};

fn request(folder: &Path, name: &str) -> Request {
    Request::new(
        Some(DownloadFolderRecord {
            path: Some(folder.to_owned()),
            revision: 3,
        }),
        folder::Claimed::default(),
        name.into(),
        None,
    )
}

#[test]
fn stalled_probe_falls_back_without_blocking_local_download_or_publication() {
    let root = std::env::temp_dir().join(format!("sage-probe-local-{}", uuid::Uuid::new_v4()));
    let remote = root.join("remote");
    let local = root.join("local");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir(&local).unwrap();
    let request = request(&remote, "report.txt");
    let fallback = request.fallback();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    let (done, completion) = mpsc::sync_channel(1);
    assert!(io::acquire(&remote).unwrap().run(
        move |destination, deadline| {
            started.send(()).unwrap();
            let _ = wait.recv();
            Ok(request.plan_on(destination, deadline))
        },
        move |planned| {
            done.send(planned.unwrap_or(fallback)).ok();
        },
        Planned::discard
    ));
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    let planned = completion.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(planned.revision, 3);
    assert!(
        matches!(planned.step, Step::Ask { ref file_name, directory: None, remember: true } if file_name == "report.txt")
    );
    let (done, completion) = mpsc::sync_channel(1);
    assert!(submit(
        self::request(&local, "report.txt"),
        move |planned| {
            done.send(planned).ok();
        }
    ));
    let Step::Silent(reservation) = completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .step
    else {
        panic!("local planning must still succeed")
    };
    std::fs::write(reservation.writer_path(), b"downloaded bytes").unwrap();
    let (done, completion) = mpsc::sync_channel(1);
    assert!(reservation.publish_off_thread(move |result| {
        done.send(result).ok();
    }));
    let path = completion
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"downloaded bytes");
    drop(release);
    worker::flush();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn saturated_probes_refuse_more_work_and_late_plans_release_their_files() {
    let executor = io::Executor::new(Duration::from_secs(2));
    let folder = std::env::temp_dir().join(format!("sage-probe-late-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&folder).unwrap();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    let mut completions = Vec::new();
    let (done, completion) = mpsc::sync_channel(1);
    let first = request(&folder, "report-0.txt");
    assert!(executor.acquire(&folder).unwrap().run(
        move |destination, deadline| {
            // Create real handles before expiring, then dispose the late result.
            let planned = first.plan_on(destination, deadline);
            assert!(matches!(&planned.step, Step::Silent(_)));
            started.send(()).unwrap();
            let _ = wait.recv();
            Ok(planned)
        },
        move |planned| {
            done.send(planned.is_err()).ok();
        },
        Planned::discard
    ));
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    completions.push(completion);
    for index in 1..crate::browser::downloads::MAX_ACTIVE_DOWNLOADS {
        let request = request(&folder, &format!("report-{index}.txt"));
        let (done, completion) = mpsc::sync_channel(1);
        assert!(executor.acquire(&folder).unwrap().run(
            move |destination, deadline| { Ok(request.plan_on(destination, deadline)) },
            move |planned| {
                done.send(planned.is_err()).ok();
            },
            Planned::discard
        ));
        completions.push(completion);
    }
    for completion in completions {
        assert!(completion.recv_timeout(Duration::from_secs(3)).unwrap());
    }
    assert!(
        executor.acquire(&folder).is_none(),
        "saturated destination must refuse another probe"
    );
    drop(release);
    // A new lease becomes available only after the late result's cleanup.
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while executor.acquire(&folder).is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    // Queue a barrier on that lane after all expired jobs and their disposal.
    let (done, completion) = mpsc::sync_channel(1);
    executor.acquire(&folder).unwrap().cleanup(move |_| {
        done.send(()).ok();
    });
    completion.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    std::fs::remove_dir(folder).unwrap();
}

#[test]
fn publication_stalled_after_planning_does_not_block_local_work() {
    let root = std::env::temp_dir().join(format!("sage-after-plan-{}", uuid::Uuid::new_v4()));
    let remote = root.join("remote");
    let local = root.join("local");
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir(&local).unwrap();
    let Step::Silent(remote_reservation) = request(&remote, "remote.txt").plan().step else {
        panic!("planning failed")
    };
    let Step::Silent(pending_local) = request(&local, "pending.txt").plan().step else {
        panic!("planning failed")
    };
    std::fs::write(remote_reservation.writer_path(), b"recoverable bytes").unwrap();
    std::fs::write(pending_local.writer_path(), b"pending local bytes").unwrap();
    let recovery = remote_reservation.writer_path().to_owned();
    // The destination becomes unresponsive only after planning succeeded.
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    assert!(io::acquire(&remote).unwrap().run(
        move |_, deadline| {
            started.send(()).unwrap();
            let _ = wait.recv();
            deadline.check()
        },
        |_| {},
        |_| {}
    ));
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    let (failed, failure) = mpsc::sync_channel(1);
    assert!(remote_reservation.publish_off_thread(move |result| {
        failed.send(result).ok();
    }));
    let (published, publication) = mpsc::sync_channel(1);
    // These coordination jobs queue in the same way as real native starts and
    // settlements. Neither may wait for destination I/O.
    assert!(worker::submit(move || {
        assert!(pending_local.publish_off_thread(move |result| {
            published.send(result).ok();
        }));
    }));
    let next = request(&local, "later.txt");
    let (planned, plan) = mpsc::sync_channel(1);
    assert!(worker::submit(move || {
        assert!(submit(next, move |result| {
            planned.send(result).ok();
        }));
    }));
    let Step::Silent(later) = plan
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap()
        .step
    else {
        panic!("local planning failed")
    };
    std::fs::write(later.writer_path(), b"later local bytes").unwrap();
    let (published, later_publication) = mpsc::sync_channel(1);
    assert!(later.publish_off_thread(move |result| {
        published.send(result).ok();
    }));
    let path = publication
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"pending local bytes");
    let path = later_publication
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"later local bytes");
    let error = failure
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "download destination I/O timed out");
    drop(release);
    worker::flush();
    assert_eq!(std::fs::read(recovery).unwrap(), b"recoverable bytes");
    assert!(!remote.join("remote.txt").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_late_plan_releases_its_claim_and_private_directory_on_its_destination() {
    let folder = std::env::temp_dir().join(format!("sage-late-plan-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&folder).unwrap();
    let request = request(&folder, "report.txt");
    let destination = io::acquire(&folder).unwrap();
    let (release, wait) = mpsc::sync_channel::<()>(0);
    let (started, start) = mpsc::sync_channel(1);
    let (done, completion) = mpsc::sync_channel(1);
    assert!(destination.run(
        move |destination, deadline| {
            let planned = request.plan_on(destination, deadline);
            assert!(matches!(&planned.step, Step::Silent(_)));
            started.send(()).unwrap();
            let _ = wait.recv();
            Ok(planned)
        },
        move |result| {
            done.send(result.is_err()).ok();
        },
        Planned::discard
    ));
    start.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(completion.recv_timeout(Duration::from_secs(3)).unwrap());
    drop(release);
    worker::flush();
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    std::fs::remove_dir(folder).unwrap();
}
