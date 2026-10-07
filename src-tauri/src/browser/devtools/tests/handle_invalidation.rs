use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
struct CountingPort(Arc<AtomicUsize>);

impl DevtoolsPort for CountingPort {
    fn navigate(&mut self, url: &str) -> Result<(String, u64), BrowserFailure> {
        Ok((url.into(), 8))
    }

    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        Ok((
            DocumentStamp {
                navigation_id: 7,
                generation: 1,
            },
            vec![NativeNode {
                backend_id: 10,
                depth: 1,
                role: "button".into(),
                name: "Continue".into(),
            }],
        ))
    }

    fn act(&mut self, _: DocumentStamp, _: u64, _: &BrowserAction) -> Result<bool, BrowserFailure> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(true)
    }

    fn capture(&mut self, _: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }

    fn matches(
        &mut self,
        _: &WaitCondition,
        _: std::time::Duration,
    ) -> Result<bool, BrowserFailure> {
        Ok(false)
    }
}

#[test]
fn navigation_and_foreign_tab_reject_native_snapshot_handle_before_act() {
    let first_scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let second_scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 2,
    };
    let first_actions = Arc::new(AtomicUsize::new(0));
    let second_actions = Arc::new(AtomicUsize::new(0));
    let mut first =
        DevtoolsCapability::new(first_scope.clone(), CountingPort(first_actions.clone()));
    let mut second =
        DevtoolsCapability::new(second_scope.clone(), CountingPort(second_actions.clone()));
    let snapshot = dispatch(
        &mut first,
        BrowserRequest {
            scope: first_scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        },
    )
    .unwrap();
    let BrowserResult::SnapshotPage { nodes, .. } = snapshot.result else {
        panic!("expected snapshot")
    };
    let handle = nodes[0].handle.clone();
    assert_eq!(
        dispatch(
            &mut second,
            BrowserRequest {
                scope: second_scope,
                command: BrowserCommand::Act {
                    action: BrowserAction::Click {
                        handle: handle.clone(),
                    },
                },
            },
        ),
        Err(BrowserFailure::StaleHandle)
    );
    assert_eq!(second_actions.load(Ordering::SeqCst), 0);
    dispatch(
        &mut first,
        BrowserRequest {
            scope: first_scope.clone(),
            command: BrowserCommand::Navigate {
                url: "https://example.com/next".into(),
            },
        },
    )
    .unwrap();
    assert_eq!(
        dispatch(
            &mut first,
            BrowserRequest {
                scope: first_scope,
                command: BrowserCommand::Act {
                    action: BrowserAction::Click { handle },
                },
            },
        ),
        Err(BrowserFailure::StaleHandle)
    );
    assert_eq!(first_actions.load(Ordering::SeqCst), 0);
}
