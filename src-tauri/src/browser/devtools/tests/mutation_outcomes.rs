use super::*;
use crate::acp::browser::{ElementHandle, FillSecret};

#[derive(Clone)]
struct MutatingPort {
    failure: Option<BrowserFailure>,
    name: Arc<Mutex<String>>,
}

impl MutatingPort {
    fn mutate(&self) -> Result<(), BrowserFailure> {
        *self.name.lock().unwrap() = "Delete account".into();
        self.failure.map_or(Ok(()), Err)
    }
}

impl DevtoolsPort for MutatingPort {
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        let (stamp, mut nodes) = FakePort.snapshot()?;
        nodes[0].name = self.name.lock().unwrap().clone();
        Ok((stamp, nodes))
    }

    fn navigate(&mut self, url: &str) -> Result<(String, u64), BrowserFailure> {
        self.mutate()?;
        Ok((url.into(), 7))
    }

    fn act(&mut self, _: DocumentStamp, _: u64, _: &BrowserAction) -> Result<bool, BrowserFailure> {
        self.mutate()?;
        Ok(true)
    }

    fn raw(&mut self, _: &str, _: &str) -> Result<String, BrowserFailure> {
        self.mutate()?;
        Ok("{}".into())
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

fn mutations(handle: ElementHandle) -> Vec<BrowserCommand> {
    let mut commands = vec![BrowserCommand::Navigate {
        url: "https://example.com/next".into(),
    }];
    for method in [
        "Runtime.evaluate",
        "Page.navigate",
        "Page.reload",
        "Page.stopLoading",
    ] {
        commands.push(BrowserCommand::Cdp {
            method: method.into(),
            params: serde_json::Map::new(),
        });
    }
    for action in [
        BrowserAction::Click {
            handle: handle.clone(),
        },
        BrowserAction::Fill {
            handle: handle.clone(),
            value: FillSecret::new("value".into()),
        },
        BrowserAction::Key {
            handle: handle.clone(),
            key: "Enter".into(),
        },
        BrowserAction::Scroll {
            handle,
            delta_x: 0,
            delta_y: 100,
        },
    ] {
        commands.push(BrowserCommand::Act { action });
    }
    commands
}

#[test]
fn mutations_retire_shared_snapshot_handles_on_every_outcome() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let request = |command| BrowserRequest {
        scope: scope.clone(),
        command,
    };
    for failure in [
        None,
        Some(BrowserFailure::Capacity),
        Some(BrowserFailure::OutcomeUnknown),
        Some(BrowserFailure::Unavailable),
        Some(BrowserFailure::Denied),
    ] {
        for index in 0..9 {
            let port = MutatingPort {
                failure,
                name: Arc::new(Mutex::new("Continue".into())),
            };
            let mut cached = DevtoolsCapability::new(scope.clone(), port);
            let BrowserResult::SnapshotPage { nodes, .. } = cached
                .perform(&request(BrowserCommand::SnapshotPage {}))
                .unwrap()
            else {
                panic!("expected snapshot")
            };
            let handle = nodes[0].handle.clone();
            let command = mutations(handle.clone()).swap_remove(index);
            let mut dispatched = cached.clone();
            let outcome = dispatched.perform(&request(command.clone()));
            assert_eq!(outcome.as_ref().err().copied(), failure, "{command:?}");
            assert_eq!(
                cached.perform(&request(BrowserCommand::Act {
                    action: BrowserAction::Click { handle },
                })),
                Err(BrowserFailure::StaleHandle),
                "{command:?}, {failure:?}"
            );
            let BrowserResult::SnapshotPage { nodes, .. } = cached
                .perform(&request(BrowserCommand::SnapshotPage {}))
                .unwrap()
            else {
                panic!("expected snapshot")
            };
            assert_eq!(nodes[0].name, "Delete account");
        }
    }
}
