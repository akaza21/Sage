use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use tokio::sync::{Notify, Semaphore};

use crate::acp::{
    contract::{ProjectionRevision, SageAgentSessionId, SageChatId, SageProjectId},
    intent::ProjectionChangedV1,
};

use super::notifier::{
    NotificationError, NotificationRecovery, NotificationSink, NotificationSinkFuture,
    ProjectionNotifier, PublishOutcome,
};

struct GateSink {
    committed: Arc<AtomicBool>,
    observed_before_commit: AtomicBool,
    entries: AtomicUsize,
    active: AtomicUsize,
    entered: Notify,
    settled: Notify,
    permits: Semaphore,
    revisions: Mutex<Vec<u64>>,
    fail: AtomicBool,
}

impl GateSink {
    fn new(committed: Arc<AtomicBool>) -> Self {
        Self {
            committed,
            observed_before_commit: AtomicBool::new(false),
            entries: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            entered: Notify::new(),
            settled: Notify::new(),
            permits: Semaphore::new(0),
            revisions: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }

    // Both waits register for the wakeup before reading the counter, so a
    // `notify_waiters` between the read and the await is never lost.
    async fn wait_for_entries(&self, expected: usize) {
        loop {
            let entered = self.entered.notified();
            tokio::pin!(entered);
            entered.as_mut().enable();
            if self.entries.load(Ordering::SeqCst) >= expected {
                return;
            }
            entered.await;
        }
    }

    fn release(&self, count: usize) {
        self.permits.add_permits(count);
    }

    async fn wait_for_idle(&self) {
        loop {
            let settled = self.settled.notified();
            tokio::pin!(settled);
            settled.as_mut().enable();
            if self.active.load(Ordering::SeqCst) == 0 {
                return;
            }
            settled.await;
        }
    }
}

impl NotificationSink for GateSink {
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            self.active.fetch_add(1, Ordering::SeqCst);
            let _active = ActiveGuard {
                active: &self.active,
                settled: &self.settled,
            };
            if !self.committed.load(Ordering::SeqCst) {
                self.observed_before_commit.store(true, Ordering::SeqCst);
            }
            lock(&self.revisions).push(event.projection_revision.get());
            self.entries.fetch_add(1, Ordering::SeqCst);
            self.entered.notify_waiters();
            self.permits
                .acquire()
                .await
                .expect("test semaphore remains open")
                .forget();
            if self.fail.load(Ordering::SeqCst) {
                Err(NotificationError::SinkUnavailable)
            } else {
                Ok(())
            }
        })
    }
}

struct ActiveGuard<'a> {
    active: &'a AtomicUsize,
    settled: &'a Notify,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
        self.settled.notify_waiters();
    }
}

fn event(project: &str, chat: &str, session: &str, revision: u64) -> ProjectionChangedV1 {
    ProjectionChangedV1 {
        project_id: SageProjectId::parse(project).unwrap(),
        chat_id: SageChatId::parse(chat).unwrap(),
        session_id: SageAgentSessionId::parse(session).unwrap(),
        projection_revision: ProjectionRevision::new(revision).unwrap(),
        child_projection_epoch: 0,
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

#[test]
fn committed_event_is_observed_only_after_commit_and_drained_once() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(GateSink::new(Arc::clone(&committed)));
        let notifier = Arc::new(ProjectionNotifier::start(sink.clone()));
        let commit_gate = Arc::new(Notify::new());
        let commit_waiting = Arc::new(Notify::new());
        let task = {
            let committed = Arc::clone(&committed);
            let notifier = Arc::clone(&notifier);
            let commit_gate = Arc::clone(&commit_gate);
            let commit_waiting = Arc::clone(&commit_waiting);
            tokio::spawn(async move {
                commit_waiting.notify_one();
                commit_gate.notified().await;
                committed.store(true, Ordering::SeqCst);
                notifier.publish_committed(event("p", "c", "s", 1))
            })
        };

        commit_waiting.notified().await;
        assert_eq!(sink.entries.load(Ordering::SeqCst), 0);
        commit_gate.notify_one();
        assert_eq!(task.await.unwrap(), Ok(PublishOutcome::Scheduled));
        sink.wait_for_entries(1).await;
        sink.release(1);
        tokio::task::yield_now().await;

        assert!(!sink.observed_before_commit.load(Ordering::SeqCst));
        assert_eq!(*lock(&sink.revisions), vec![1]);
        notifier.shutdown().await;
    });
}

#[test]
fn a_child_epoch_at_an_unchanged_revision_is_still_delivered() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());
        let child = |epoch| ProjectionChangedV1 {
            child_projection_epoch: epoch,
            ..event("p", "c", "s", 4)
        };
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 4)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.wait_for_entries(1).await;
        // Keep the original revision in flight so only the newer child epoch
        // can admit this second event at the unchanged revision.
        assert_eq!(
            notifier.publish_committed(child(7)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.release(1);
        sink.wait_for_entries(2).await;
        // A repeat of the child epoch being delivered coalesces into it. Once
        // that delivery settles the notifier is idle and would schedule the
        // repeat again, so publish it while the delivery is still held.
        assert_eq!(
            notifier.publish_committed(child(7)),
            Ok(PublishOutcome::Coalesced)
        );
        sink.release(1);
        sink.wait_for_idle().await;
        assert_eq!(*lock(&sink.revisions), vec![4, 4]);
        notifier.shutdown().await;
    });
}

#[test]
fn same_tuple_coalesces_to_greatest_revision_in_one_pending_slot() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());

        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 1)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.wait_for_entries(1).await;
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 3)),
            Ok(PublishOutcome::Scheduled)
        );
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 2)),
            Ok(PublishOutcome::Coalesced)
        );
        assert_eq!(notifier.scheduled_wakes(), 1);

        sink.release(2);
        sink.wait_for_entries(2).await;
        tokio::task::yield_now().await;
        assert_eq!(*lock(&sink.revisions), vec![1, 3]);
        assert_eq!(notifier.scheduled_wakes(), 1);
        notifier.shutdown().await;
    });
}

#[test]
fn an_in_flight_greatest_revision_rejects_duplicate_and_older_revisions() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());

        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 3)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.wait_for_entries(1).await;
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 3)),
            Ok(PublishOutcome::Coalesced)
        );
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 2)),
            Ok(PublishOutcome::Coalesced)
        );

        sink.release(1);
        tokio::task::yield_now().await;
        assert_eq!(*lock(&sink.revisions), vec![3]);
        assert_eq!(notifier.scheduled_wakes(), 1);
        notifier.shutdown().await;
    });
}

#[test]
fn a_different_tuple_forces_bootstrap_without_overwriting() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());

        notifier
            .publish_committed(event("p-one", "c-one", "s-one", 1))
            .unwrap();
        sink.wait_for_entries(1).await;
        assert_eq!(
            notifier.publish_committed(event("p-two", "c-two", "s-two", 8)),
            Ok(PublishOutcome::BootstrapRequired)
        );
        assert_eq!(
            notifier.recovery(),
            Some(NotificationRecovery::TupleContention)
        );
        let recovery_wait = notifier.wait_for_recovery();
        tokio::pin!(recovery_wait);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), recovery_wait.as_mut())
                .await
                .is_err(),
            "in-flight publication keeps recovery non-actionable"
        );
        assert!(notifier.acknowledge_authoritative_bootstrap());

        sink.release(1);
        sink.wait_for_idle().await;
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), recovery_wait.as_mut()).await,
            Ok(None),
            "settled acknowledgement must wake the existing recovery waiter"
        );
        assert_eq!(*lock(&sink.revisions), vec![1]);
        assert_eq!(notifier.recovery(), None);
        assert_eq!(
            notifier.publish_committed(event("p-two", "c-two", "s-two", 9)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.wait_for_entries(2).await;
        sink.release(1);
        notifier.shutdown().await;
    });
}

#[test]
fn project_switch_suppresses_background_notices_without_stopping_the_turn() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());
        notifier.set_active_project(SageProjectId::parse("p-new").unwrap());

        assert_eq!(
            notifier.publish_committed(event("p-old", "c-old", "s-old", 9)),
            Ok(PublishOutcome::SuppressedInactive)
        );
        assert_eq!(sink.entries.load(Ordering::SeqCst), 0);
        assert_eq!(notifier.recovery(), None);

        assert_eq!(
            notifier.publish_committed(event("p-new", "c-new", "s-new", 1)),
            Ok(PublishOutcome::Scheduled)
        );
        sink.wait_for_entries(1).await;
        sink.release(1);
        tokio::task::yield_now().await;
        assert_eq!(*lock(&sink.revisions), vec![1]);
        assert_eq!(notifier.recovery(), None);
        notifier.shutdown().await;
    });
}

#[test]
fn sink_failure_is_not_retried_and_requires_authoritative_bootstrap() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        sink.fail.store(true, Ordering::SeqCst);
        let notifier = ProjectionNotifier::start(sink.clone());

        notifier.publish_committed(event("p", "c", "s", 1)).unwrap();
        sink.wait_for_entries(1).await;
        sink.release(1);
        assert_eq!(
            notifier.wait_for_recovery().await,
            Some(NotificationRecovery::SinkFailure)
        );
        assert_eq!(*lock(&sink.revisions), vec![1]);
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 2)),
            Ok(PublishOutcome::RecoveryRequired(
                NotificationRecovery::SinkFailure
            ))
        );
        assert_eq!(*lock(&sink.revisions), vec![1]);
        assert!(notifier.acknowledge_authoritative_bootstrap());
        notifier.shutdown().await;
    });
}

#[test]
fn shutdown_cancels_the_only_worker_and_rejects_later_publication() {
    runtime().block_on(async {
        let committed = Arc::new(AtomicBool::new(true));
        let sink = Arc::new(GateSink::new(committed));
        let notifier = ProjectionNotifier::start(sink.clone());
        notifier.publish_committed(event("p", "c", "s", 1)).unwrap();
        sink.wait_for_entries(1).await;

        notifier.shutdown().await;
        assert_eq!(
            notifier.publish_committed(event("p", "c", "s", 2)),
            Ok(PublishOutcome::Shutdown)
        );
        assert_eq!(sink.entries.load(Ordering::SeqCst), 1);
        assert_eq!(sink.active.load(Ordering::SeqCst), 0);
    });
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn a_child_only_notice_is_routed_away_from_the_parent_page_reload() {
    use crate::acp::intent::{ProjectionNoticeRoutes, route_projection_notice};
    let mut last = ProjectionNoticeRoutes::default();
    let child = |revision, epoch| ProjectionChangedV1 {
        child_projection_epoch: epoch,
        ..event("p", "c", "s", revision)
    };
    // Nothing published yet: even a child change reloads the page once.
    assert!(route_projection_notice(&mut last, child(4, 7)).is_ok());
    // Same revision, new child epoch: a child notice only.
    let routed = route_projection_notice(&mut last, child(4, 8)).unwrap_err();
    assert_eq!(routed.child_projection_epoch, 8);
    assert_eq!(routed.chat_id.as_str(), "c");
    // A moved revision is a parent change again.
    assert!(route_projection_notice(&mut last, child(5, 9)).is_ok());
    assert!(route_projection_notice(&mut last, event("p", "c", "s", 6)).is_ok());
    // Another chat at the same revision is a parent change for that chat.
    let other = ProjectionChangedV1 {
        child_projection_epoch: 10,
        ..event("p", "other", "s", 6)
    };
    assert!(route_projection_notice(&mut last, other).is_ok());
}

#[test]
fn interleaved_chats_keep_their_own_child_only_routing() {
    use crate::acp::intent::{
        MAX_PROJECTION_NOTICE_ROUTES, ProjectionNoticeRoutes, route_projection_notice,
    };
    let mut routes = ProjectionNoticeRoutes::default();
    let child = |chat: &str, revision, epoch| ProjectionChangedV1 {
        child_projection_epoch: epoch,
        ..event("p", chat, "s", revision)
    };
    assert!(route_projection_notice(&mut routes, child("a", 4, 1)).is_ok());
    assert!(route_projection_notice(&mut routes, child("b", 9, 1)).is_ok());
    // Chat b's notice in between does not make a's child change a reload.
    assert!(route_projection_notice(&mut routes, child("a", 4, 2)).is_err());
    assert!(route_projection_notice(&mut routes, child("b", 9, 2)).is_err());
    // Another session of the same chat is its own route.
    let other_session = ProjectionChangedV1 {
        child_projection_epoch: 3,
        ..event("p", "a", "s2", 4)
    };
    assert!(route_projection_notice(&mut routes, other_session).is_ok());
    // The routes are a bounded LRU: touching a keeps it, b is evicted first.
    for index in 0..MAX_PROJECTION_NOTICE_ROUTES - 2 {
        let chat = format!("chat-{index}");
        assert!(route_projection_notice(&mut routes, child(&chat, 1, 1)).is_ok());
        if index == 0 {
            assert!(route_projection_notice(&mut routes, child("a", 4, 4)).is_err());
        }
    }
    assert!(route_projection_notice(&mut routes, child("a", 4, 5)).is_err());
    assert!(
        route_projection_notice(&mut routes, child("b", 9, 3)).is_ok(),
        "an evicted route reloads the parent page once"
    );
}
