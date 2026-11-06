use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::OptionalExtension;
use tauri::Manager;

use crate::acp::{
    browser::{
        mcp::{
            BrowserMcpServer,
            transport::{BrowserMcpPipeHost, BrowserMcpSessionState},
        },
        session_capability::SessionBrowserCapability,
    },
    mcp::McpAdmission,
    scheduler::RuntimeOwner,
};

pub(super) struct BrowserMcpActivationGuard {
    revoked: Arc<AtomicBool>,
    committed: bool,
}

impl BrowserMcpActivationGuard {
    #[cfg(test)]
    pub(super) fn for_test() -> (Self, Arc<AtomicBool>) {
        let revoked = Arc::new(AtomicBool::new(false));
        (
            Self {
                revoked: Arc::clone(&revoked),
                committed: false,
            },
            revoked,
        )
    }

    pub(super) fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for BrowserMcpActivationGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.revoked.store(true, Ordering::SeqCst);
        }
    }
}

pub(super) fn session_state(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
) -> BrowserMcpSessionState {
    let row = database
        .connection()
        .query_row(
            "SELECT s.state,e.state FROM agent_sessions s
         LEFT JOIN session_runtime_epochs e ON e.session_id=s.id AND e.runtime_epoch=?4
         WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .optional();
    match row {
        Ok(Some((session, Some(epoch))))
            // Resume creates a starting epoch under a recovering session before
            // the harness connects. Keep its new pipe alive just as on first start.
            if matches!(session.as_str(), "starting" | "recovering" | "ready" | "active")
                && matches!(epoch.as_str(), "starting" | "running") =>
        {
            BrowserMcpSessionState::Live
        }
        Ok(Some((session, None)))
            if matches!(
                session.as_str(),
                "created" | "starting" | "ready" | "active"
            ) =>
        {
            BrowserMcpSessionState::Pending
        }
        _ => BrowserMcpSessionState::Ended,
    }
}

impl super::AcpService {
    pub(super) fn prepare_browser_mcp(
        &self,
        owner: &RuntimeOwner,
    ) -> (McpAdmission, Option<BrowserMcpActivationGuard>) {
        let Some(app) = self.browser_app.lock().ok().and_then(|slot| slot.clone()) else {
            return (McpAdmission::default(), None);
        };
        let Ok(host) = BrowserMcpPipeHost::bind() else {
            return (McpAdmission::default(), None);
        };
        let Ok(executable) = std::env::current_exe() else {
            return (McpAdmission::default(), None);
        };
        let Some(command) = executable.to_str().map(str::to_owned) else {
            return (McpAdmission::default(), None);
        };
        let port = SessionBrowserCapability::new(
            app.clone(),
            owner.project_id.clone(),
            owner.chat_id.clone(),
            owner.session_id.clone(),
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| u64::try_from(time.as_millis()).ok())
            .unwrap_or_default();
        let (mut server, token) =
            BrowserMcpServer::issue_for_session(port, owner.session_id.as_str().to_owned(), now);
        let settings = app.state::<crate::state::AppState>().settings.clone();
        server.set_developer_tools(Box::new(move || {
            settings.browser_developer_mode().unwrap_or(false)
        }));
        server.set_operation_observer(Box::new(
            super::browser_operation::ServiceBrowserOperationObserver::new(app, owner.clone()),
        ));
        let environment = host.child_environment(owner.session_id.as_str(), token);
        let Ok(admission) = McpAdmission::browser_stdio(command, environment) else {
            return (McpAdmission::default(), None);
        };
        let database = self.database.clone();
        let listener_owner = owner.clone();
        let revoked = Arc::new(AtomicBool::new(false));
        let listener_revoked = Arc::clone(&revoked);
        let session_id = owner.session_id.clone();
        let name = format!(
            "sage-browser-mcp-{}",
            &session_id.as_str()[..session_id.as_str().len().min(16)]
        );
        if std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                let _ = host.serve(&mut server, session_id.as_str(), || {
                    if listener_revoked.load(Ordering::SeqCst) {
                        BrowserMcpSessionState::Ended
                    } else {
                        session_state(&database, &listener_owner)
                    }
                });
            })
            .is_err()
        {
            return (McpAdmission::default(), None);
        }
        (
            admission,
            Some(BrowserMcpActivationGuard {
                revoked,
                committed: false,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_activation_revokes_the_pending_listener() {
        let revoked = Arc::new(AtomicBool::new(false));
        {
            let _guard = BrowserMcpActivationGuard {
                revoked: Arc::clone(&revoked),
                committed: false,
            };
        }
        assert!(revoked.load(Ordering::SeqCst));
    }

    #[test]
    fn successful_activation_keeps_the_listener_live() {
        let revoked = Arc::new(AtomicBool::new(false));
        {
            let mut guard = BrowserMcpActivationGuard {
                revoked: Arc::clone(&revoked),
                committed: false,
            };
            guard.commit();
        }
        assert!(!revoked.load(Ordering::SeqCst));
    }
}
