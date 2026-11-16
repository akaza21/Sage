use super::*;
use crate::workspace_notify::{WorkspaceNotifyNotice, batch::NoticeSink};
use std::{sync::mpsc, time::Duration};

fn notice(root: &str, path: &str) -> WorkspaceNotifyNotice {
    WorkspaceNotifyNotice {
        project_id: "project".into(),
        project_generation: 1,
        transition_epoch: 1,
        root_id: root.into(),
        relative_path: path.into(),
        kind: "changed".into(),
        git_relevant: None,
    }
}

#[test]
fn blocked_git_delivers_listings_and_other_roots_before_its_budget_expires() {
    let root = sage_test_support::test_paths::temp_dir().join(uuid::Uuid::new_v4().to_string());
    let refresh = Arc::new(GitRefresh::new(vec![
        ("slow".into(), root.join("slow")),
        ("fast".into(), root.join("fast")),
    ]));
    let (sent, received) = mpsc::channel();
    let (started, running) = mpsc::channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let input = vec![notice("slow", "src/a.rs"), notice("fast", "src/b.rs")];
    let sink = Arc::new(NoticeSink::default());
    for notice in &input {
        sink.push_record(notice.clone(), &notice.relative_path, false, false);
    }
    let emitter = {
        let refresh = refresh.clone();
        let cancelled = cancelled.clone();
        let sink = sink.clone();
        let release = release.clone();
        std::thread::spawn(move || {
            let emit: Arc<dyn Fn(WorkspaceNotifyBatch) + Send + Sync> = Arc::new(move |batch| {
                sent.send(batch).unwrap();
            });
            sink.run_emitter(
                &cancelled,
                || std::thread::sleep(Duration::from_millis(1)),
                |batch, windows| {
                    refresh.emit_with(
                        batch,
                        windows,
                        &cancelled,
                        emit.clone(),
                        Duration::from_millis(500),
                        {
                            let release = release.clone();
                            let started = started.clone();
                            move |_, path, _| {
                                let release = release.clone();
                                let started = started.clone();
                                async move {
                                    if path.ends_with("slow") {
                                        started.send(()).unwrap();
                                        release.notified().await;
                                    }
                                    Ok(false)
                                }
                            }
                        },
                    )
                },
            );
        })
    };
    let listing = received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        listing
            .notices
            .iter()
            .map(|n| n.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["src/a.rs", "src/b.rs"]
    );
    assert!(
        listing
            .notices
            .iter()
            .all(|n| n.git_relevant == Some(false))
    );
    running
        .recv_timeout(sage_test_support::test_wait::HANG_GUARD)
        .unwrap();
    // Another window must remain prompt while the first query still owns
    // this root. It conservatively refreshes Git instead of queuing a query.
    let next = notice("slow", "src/later.rs");
    sink.push_record(next.clone(), &next.relative_path, false, false);
    let mut delivered = Vec::new();
    for _ in 0..3 {
        delivered.extend(
            received
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .notices,
        );
    }
    // Independent roots and the timer can race; compare the delivered
    // notices without imposing an ordering on their asynchronous decisions.
    delivered.sort_by(|a, b| (&a.root_id, &a.relative_path).cmp(&(&b.root_id, &b.relative_path)));
    assert_eq!(
        delivered,
        [
            WorkspaceNotifyNotice {
                kind: "repository".into(),
                ..input[1].clone()
            },
            WorkspaceNotifyNotice {
                kind: "repository".into(),
                ..input[0].clone()
            },
            next,
        ]
    );

    // Expiry emits the conservative decision but retains the root's process
    // owner until the runner has actually completed its cleanup.
    let gate = refresh.roots["slow"].gate.as_ref().unwrap();
    assert!(gate.try_lock().is_err());
    release.notify_one();
    sage_core::rt::block_on(async {
        let _settled = tokio::time::timeout(sage_test_support::test_wait::HANG_GUARD, gate.lock())
            .await
            .unwrap();
    });
    cancelled.store(true, Ordering::Release);
    emitter.join().unwrap();
    assert!(received.try_recv().is_err());
}
