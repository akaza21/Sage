use super::{
    BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult, BrowserScope,
    ElementHandle, WaitCondition,
};
use std::collections::HashMap;

#[derive(Default)]
pub struct FakeBrowserCapability {
    tabs: HashMap<u64, (String, u64)>,
    handles: HashMap<String, HandleBinding>,
    next_handle: u64,
    actions: usize,
}

struct HandleBinding {
    session_id: String,
    tab_id: u64,
    navigation_epoch: u64,
    node_id: u64,
}

impl FakeBrowserCapability {
    pub fn open_tab(&mut self, scope: BrowserScope) {
        self.handles
            .retain(|_, binding| binding.tab_id != scope.tab_id);
        self.tabs.insert(scope.tab_id, (scope.session_id, 1));
    }

    pub fn issue_handle(&mut self, scope: &BrowserScope) -> Result<ElementHandle, BrowserFailure> {
        let epoch = self.epoch(scope)?;
        self.next_handle += 1;
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.handles.insert(
            token.clone(),
            HandleBinding {
                session_id: scope.session_id.clone(),
                tab_id: scope.tab_id,
                navigation_epoch: epoch,
                node_id: self.next_handle,
            },
        );
        Ok(ElementHandle::from_token(token))
    }

    pub fn advance_navigation(&mut self, scope: &BrowserScope) -> Result<u64, BrowserFailure> {
        let tab = self
            .tabs
            .get_mut(&scope.tab_id)
            .ok_or(BrowserFailure::Unavailable)?;
        if tab.0 != scope.session_id {
            return Err(BrowserFailure::Denied);
        }
        tab.1 += 1;
        Ok(tab.1)
    }

    pub fn action_count(&self) -> usize {
        self.actions
    }

    fn epoch(&self, scope: &BrowserScope) -> Result<u64, BrowserFailure> {
        let tab = self
            .tabs
            .get(&scope.tab_id)
            .ok_or(BrowserFailure::Unavailable)?;
        if tab.0 != scope.session_id {
            return Err(BrowserFailure::Denied);
        }
        Ok(tab.1)
    }

    fn check_handle(
        &self,
        scope: &BrowserScope,
        handle: &ElementHandle,
    ) -> Result<(), BrowserFailure> {
        let epoch = self.epoch(scope)?;
        match self.handles.get(handle.token()) {
            Some(binding)
                if binding.session_id == scope.session_id
                    && binding.tab_id == scope.tab_id
                    && binding.navigation_epoch == epoch
                    && binding.node_id != 0 =>
            {
                Ok(())
            }
            _ => Err(BrowserFailure::StaleHandle),
        }
    }
}

impl BrowserCapability for FakeBrowserCapability {
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        if let BrowserCommand::OpenTab { .. } = &request.command {
            let tab_id = self.tabs.keys().copied().max().unwrap_or(0) + 1;
            self.open_tab(BrowserScope {
                session_id: request.scope.session_id.clone(),
                tab_id,
            });
            return Ok(BrowserResult::OpenTab { tab_id });
        }
        self.epoch(&request.scope)?;
        match &request.command {
            BrowserCommand::OpenTab { .. } => unreachable!(),
            BrowserCommand::Navigate { url } => Ok(BrowserResult::Navigate {
                url: url.clone(),
                navigation_epoch: self.advance_navigation(&request.scope)?,
            }),
            BrowserCommand::SnapshotPage {} => Ok(BrowserResult::SnapshotPage {
                navigation_epoch: self.epoch(&request.scope)?,
                text: String::new(),
                nodes: vec![super::SnapshotNode {
                    handle: self.issue_handle(&request.scope)?,
                    role: "document".into(),
                    name: String::new(),
                }],
            }),
            BrowserCommand::Act { action } => {
                self.check_handle(&request.scope, action.handle())?;
                self.actions += 1;
                Ok(BrowserResult::Act { applied: true })
            }
            BrowserCommand::WaitFor { condition, .. } => {
                if let WaitCondition::Handle { handle } = condition {
                    self.check_handle(&request.scope, handle)?;
                }
                Ok(BrowserResult::WaitFor { matched: true })
            }
            BrowserCommand::Screenshot { .. } => Err(BrowserFailure::Unavailable),
            BrowserCommand::ReadConsole { .. } => Ok(BrowserResult::ReadConsole {
                next_cursor: 0,
                entries: vec![],
            }),
            BrowserCommand::ReadNetwork { .. } => Ok(BrowserResult::ReadNetwork {
                next_cursor: 0,
                entries: vec![],
            }),
        }
    }
}
