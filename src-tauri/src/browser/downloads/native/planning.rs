//! Bounded, deadline-limited filesystem planning, separate from publication.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use super::choice::Planned;

const MAX_PROBES: usize = 2;
const DEADLINE: Duration = Duration::from_secs(2);

struct Planner {
    active: Arc<AtomicUsize>,
    deadline: Duration,
}

struct Permit(Arc<AtomicUsize>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn submit(
    probe: impl FnOnce() -> Planned + Send + 'static,
    completed: impl FnOnce(Option<Planned>) + Send + 'static,
) -> bool {
    static PLANNER: OnceLock<Planner> = OnceLock::new();
    PLANNER
        .get_or_init(|| Planner::new(DEADLINE))
        .submit(probe, completed)
}

impl Planner {
    fn new(deadline: Duration) -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
            deadline,
        }
    }

    /// No queue. A timed-out probe keeps its permit until the OS call returns,
    /// so permanently stuck calls cannot accumulate threads or retained state.
    fn submit(
        &self,
        probe: impl FnOnce() -> Planned + Send + 'static,
        completed: impl FnOnce(Option<Planned>) + Send + 'static,
    ) -> bool {
        if self
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < MAX_PROBES).then_some(active + 1)
            })
            .is_err()
        {
            return false;
        }
        let permit = Arc::new(Permit(Arc::clone(&self.active)));
        let deadline = Instant::now() + self.deadline;
        std::thread::Builder::new()
            .name("download-plan-deadline".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                // A rendezvous channel leaves late results on the isolated thread,
                // including their cleanup. Only the deadline thread owns the UI callback.
                let (sender, receiver) = mpsc::sync_channel(0);
                let probe_permit = Arc::clone(&permit);
                let spawned = std::thread::Builder::new()
                    .name("download-folder-probe".into())
                    .stack_size(256 * 1024)
                    .spawn(move || {
                        let _permit = probe_permit;
                        let planned = probe();
                        if let Err(late) = sender.send(planned) {
                            late.0.discard();
                        }
                    })
                    .is_ok();
                let planned = if spawned {
                    receiver
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .ok()
                } else {
                    None
                };
                // Disconnect before invoking the callback: timeout cannot race a late
                // result into the UI or the shared publication/cleanup worker.
                drop(receiver);
                completed(planned);
                // Both the probe and its completion must finish before another
                // pair can start, bounding deadline threads as well as OS calls.
                drop(permit);
            })
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, time::Duration};

    use super::super::choice::{Request, Step};
    use super::*;
    use crate::browser::downloads::{folder, worker};
    use crate::settings_store::DownloadFolderRecord;

    #[test]
    fn stalled_probe_falls_back_without_blocking_local_download_or_publication() {
        let planner = Planner::new(DEADLINE);
        let (started, start) = mpsc::sync_channel(1);
        let (completed, completion) = mpsc::sync_channel(1);
        let request = Request::new(
            Some(DownloadFolderRecord {
                path: Some(std::env::temp_dir()),
                revision: 3,
            }),
            folder::Claimed::default(),
            "report.txt".into(),
            None,
        );
        let fallback = request.fallback();
        assert!(planner.submit(
            move || {
                started.send(()).unwrap();
                loop {
                    std::thread::park();
                }
            },
            move |plan| {
                completed.send(plan.unwrap_or(fallback)).ok();
            }
        ));
        start.recv_timeout(Duration::from_secs(1)).unwrap();
        let planned = completion
            .recv_timeout(DEADLINE + Duration::from_secs(1))
            .expect("deadline must choose the picker fallback");
        assert_eq!(planned.revision, 3);
        assert!(
            matches!(planned.step, Step::Ask { ref file_name, directory: None, remember: true } if file_name == "report.txt")
        );

        let path = std::env::temp_dir().join(format!("sage-probe-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        let local = Request::new(
            Some(DownloadFolderRecord {
                path: Some(path.clone()),
                revision: 4,
            }),
            folder::Claimed::default(),
            "report.txt".into(),
            None,
        );
        let (completed, completion) = mpsc::sync_channel(1);
        assert!(planner.submit(
            move || local.plan(),
            move |plan| {
                completed.send(plan).ok();
            }
        ));
        let Step::Silent(reservation) = completion
            .recv_timeout(DEADLINE + Duration::from_secs(1))
            .unwrap()
            .unwrap()
            .step
        else {
            panic!("local planning must still succeed");
        };
        std::fs::write(reservation.writer_path(), b"downloaded bytes").unwrap();
        let (published, publication) = mpsc::sync_channel(1);
        assert!(worker::submit(move || {
            published.send(reservation.publish()).unwrap();
        }));
        let published = publication
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .unwrap();
        assert_eq!(std::fs::read(published).unwrap(), b"downloaded bytes");
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn saturated_probes_refuse_more_work_and_late_plans_release_their_files() {
        let planner = Planner::new(Duration::from_millis(100));
        let folder = std::env::temp_dir().join(format!("sage-probe-late-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&folder).unwrap();
        let mut releases = Vec::new();
        let mut joins = Vec::new();
        for index in 0..MAX_PROBES {
            let request = Request::new(
                Some(DownloadFolderRecord {
                    path: Some(folder.clone()),
                    revision: 0,
                }),
                folder::Claimed::default(),
                format!("report-{index}.txt"),
                None,
            );
            let (release, wait) = mpsc::sync_channel(0);
            let (done, completion) = mpsc::sync_channel(1);
            assert!(planner.submit(
                move || {
                    wait.recv().unwrap();
                    request.plan()
                },
                move |plan| {
                    done.send(plan.is_none()).ok();
                }
            ));
            releases.push(release);
            joins.push(completion);
        }
        for completion in joins {
            assert!(completion.recv_timeout(Duration::from_secs(1)).unwrap());
        }
        assert!(!planner.submit(
            || unreachable!("saturated probe must never run"),
            |_| unreachable!()
        ));
        for release in releases {
            release.send(()).unwrap();
        }
        // Synchronize on probe resource release, then inspect real file output.
        let deadline = Instant::now() + Duration::from_secs(2);
        while planner.active.load(Ordering::Acquire) != 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        std::fs::remove_dir(folder).unwrap();
    }
}
