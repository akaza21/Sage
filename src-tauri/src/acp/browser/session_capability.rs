use tauri::{AppHandle, Emitter, Manager, Runtime};
mod open_tab_commit;

use crate::{
    acp::{
        browser::{
            BrowserCapability, BrowserFailure, BrowserRequest, BrowserResult,
            attachment::{AttachmentLease, MAX_AGENT_TABS_PER_SESSION},
            permission::{
                BrowserAuthority, BrowserCapabilityDispatch, BrowserDispatchPermit,
                BrowserPermissionGate, PendingBrowserResult,
            },
        },
        contract::{SageAgentSessionId, SageChatId, SageProjectId},
        service::browser_permission::{BrowserPermissionOwner, ServiceBrowserPermissionDecider},
    },
    browser::{BrowserState, devtools::NativeDevtools},
    state::AppState,
};

pub(crate) struct SessionBrowserCapability {
    app: AppHandle,
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    native: Option<(AttachmentLease, NativeDevtools)>,
}

struct OpenTabPort {
    app: AppHandle,
    owner: BrowserPermissionOwner,
}

pub(crate) fn with_session_owner_guards<R: Runtime, P, D>(
    gate: BrowserPermissionGate<P, D>,
    app: AppHandle<R>,
    owner: BrowserPermissionOwner,
) -> BrowserPermissionGate<P, D> {
    let precheck_app = app.clone();
    let precheck_owner = owner.clone();
    gate.with_owner_guard(move || {
        session_owner_precheck(&precheck_app.state::<AppState>().acp, &precheck_owner)
    })
    .with_owner_witness(move || session_owner_witness(&app.state::<AppState>().acp, &owner))
}

pub(crate) fn session_owner_precheck(
    service: &crate::acp::service::AcpService,
    owner: &BrowserPermissionOwner,
) -> bool {
    service.browser_owner_precheck_matches(owner)
}

pub(crate) fn session_owner_witness(
    service: &crate::acp::service::AcpService,
    owner: &BrowserPermissionOwner,
) -> bool {
    service.browser_owner_witness_matches(owner)
}

impl BrowserCapabilityDispatch for OpenTabPort {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let crate::acp::browser::BrowserCommand::OpenTab { url } = &request.command else {
            return Err(BrowserFailure::InvalidRequest);
        };
        let app = self.app.clone();
        let owner = self.owner.clone();
        let url = url.clone();
        Ok(Box::new(move || {
            let service = &app.state::<AppState>().acp;
            if !service.browser_owner_precheck_matches(&owner) {
                return Err(BrowserFailure::Denied);
            }
            let user_controls =
                || service.browser_chat_under_user_control(&owner.project_id, &owner.chat_id);
            permit.with_live_lock(|| {
                if !service.browser_owner_witness_matches(&owner) {
                    return Err(BrowserFailure::Denied);
                }
                // The user may have taken over a tab of this chat since the
                // call was admitted.
                if user_controls() {
                    return Err(BrowserFailure::UserControl);
                }
                permit.mark_issued();
                Ok(())
            })?;
            let browser = app.state::<BrowserState>();
            let label = tauri::async_runtime::block_on(
                crate::browser::lifecycle::open_agent_browser_tab(
                    app.clone(),
                    browser,
                    url.clone(),
                ),
            )
            .map_err(|error| {
                if error == "browser tab limit reached" || error == "browser tab labels exhausted" {
                    BrowserFailure::Capacity
                } else {
                    BrowserFailure::Unavailable
                }
            })?;
            let settled = (0..400).find_map(|_| {
                if !permit.is_live() {
                    return None;
                }
                let result = app.state::<BrowserState>().settled_tab(&label).ok();
                if result.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                result
            });
            let close = || {
                tauri::async_runtime::block_on(crate::browser::lifecycle::browser_close(
                    app.clone(),
                    app.state::<BrowserState>(),
                    label.clone(),
                ))
                .map_err(|_| BrowserFailure::OutcomeUnknown)
            };
            let (actual_url, revision) = match settled {
                Some(value) => value,
                None => {
                    close()?;
                    return Err(permit.revoked_failure());
                }
            };
            let Some(tab_id) = label
                .strip_prefix("browser-")
                .and_then(|value| value.parse::<u64>().ok())
            else {
                close()?;
                return Err(BrowserFailure::Unavailable);
            };
            let service = &app.state::<AppState>().acp;
            open_tab_commit::complete_opened_tab(
                || {
                    let owner_current = permit.is_live()
                        && service
                            .current_browser_owner(
                                &owner.project_id,
                                &owner.chat_id,
                                &owner.session_id,
                            )
                            .is_some_and(|current| {
                                current.turn_id == owner.turn_id
                                    && current.runtime_epoch == owner.runtime_epoch
                                    && current.binding_digest == owner.binding_digest
                            });
                    if !owner_current {
                        Err(BrowserFailure::Denied)
                    } else if user_controls() {
                        Err(BrowserFailure::UserControl)
                    } else {
                        Ok(())
                    }
                },
                || {
                    if service.agent_browser_count(
                        &owner.project_id,
                        &owner.chat_id,
                        owner.session_id.as_str(),
                    ) >= MAX_AGENT_TABS_PER_SESSION
                    {
                        return Err(BrowserFailure::Capacity);
                    }
                    service
                        .attach_agent_browser_tab(&owner, &label, &actual_url, revision)
                        .map_err(|_| BrowserFailure::Denied)
                },
                || {
                    service.detach_agent_browser_tab(
                        &owner.project_id,
                        &owner.chat_id,
                        owner.session_id.as_str(),
                        tab_id,
                    );
                },
                || {
                    service.retire_agent_browser_tab(
                        &owner.project_id,
                        &owner.chat_id,
                        owner.session_id.as_str(),
                        tab_id,
                    );
                },
                || {
                    let event = serde_json::json!({"projectId":owner.project_id.as_str(),"tabId":label,"url":actual_url});
                    app.emit_to("main", "browser:agent-opened", event)
                        .map_err(|_| BrowserFailure::Unavailable)
                },
                close,
            )?;
            Ok(BrowserResult::OpenTab { tab_id })
        }))
    }
}

impl SessionBrowserCapability {
    pub(crate) fn new(
        app: AppHandle,
        project_id: SageProjectId,
        chat_id: SageChatId,
        session_id: SageAgentSessionId,
    ) -> Self {
        Self {
            app,
            project_id,
            chat_id,
            session_id,
            native: None,
        }
    }

    fn user_holds_a_tab(&self) -> bool {
        self.app
            .state::<AppState>()
            .acp
            .browser_chat_under_user_control(&self.project_id, &self.chat_id)
    }

    fn perform_once(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        if request.scope.session_id != self.session_id.as_str() {
            return Err(BrowserFailure::Denied);
        }
        let app_state = self.app.state::<AppState>();
        if let crate::acp::browser::BrowserCommand::OpenTab { url } = &request.command {
            // While the user holds one of this chat's tabs the Agent waits;
            // a parallel tab would route around the pause.
            if app_state
                .acp
                .browser_chat_under_user_control(&self.project_id, &self.chat_id)
            {
                return Err(BrowserFailure::UserControl);
            }
            if app_state.acp.agent_browser_count(
                &self.project_id,
                &self.chat_id,
                self.session_id.as_str(),
            ) >= MAX_AGENT_TABS_PER_SESSION
            {
                return Err(BrowserFailure::Capacity);
            }
            let owner = app_state
                .acp
                .current_browser_owner(&self.project_id, &self.chat_id, &self.session_id)
                .ok_or(BrowserFailure::Denied)?;
            let authority = BrowserAuthority::attached(request.scope.clone(), url, 1)?;
            let decider = ServiceBrowserPermissionDecider {
                service: &app_state.acp,
                owner: owner.clone(),
                authority: authority.clone(),
                attachment: None,
            };
            let port = OpenTabPort {
                app: self.app.clone(),
                owner,
            };
            return BrowserPermissionGate::new(port, decider, authority).perform(request);
        }
        let lease = app_state
            .acp
            .browser_attachment(&self.project_id, &self.chat_id)
            .filter(|lease| lease.tab_number == request.scope.tab_id)
            .or_else(|| {
                app_state.acp.agent_browser_attachment(
                    &self.project_id,
                    &self.chat_id,
                    self.session_id.as_str(),
                    request.scope.tab_id,
                )
            })
            .ok_or(BrowserFailure::NoAttachment)?;
        if request.scope.tab_id != lease.tab_number {
            return Err(BrowserFailure::Denied);
        }
        let browser = self.app.state::<BrowserState>();
        let (url, revision) = browser
            .settled_tab(&lease.tab_label)
            .map_err(|_| BrowserFailure::Denied)?;
        if url != lease.url || revision != lease.tab_revision {
            return Err(BrowserFailure::Denied);
        }
        let authority = lease
            .authority_for(&request.scope)
            .ok_or(if lease.user_controls() {
                BrowserFailure::UserControl
            } else {
                BrowserFailure::Denied
            })?;
        let owner = app_state
            .acp
            .current_browser_owner(&self.project_id, &self.chat_id, &self.session_id)
            .ok_or(BrowserFailure::Denied)?;
        if !self
            .native
            .as_ref()
            .is_some_and(|(cached, _)| cached.same_attachment(&lease) && cached.is_live())
        {
            self.native = Some((
                lease.clone(),
                NativeDevtools::for_tab(&browser, request.scope.clone())?,
            ));
        }
        let native = &mut self.native.as_mut().ok_or(BrowserFailure::Unavailable)?.1;
        let label = lease.tab_label.clone();
        let decider = ServiceBrowserPermissionDecider {
            service: &app_state.acp,
            owner: owner.clone(),
            authority: authority.clone(),
            attachment: Some(lease),
        };
        let app = self.app.clone();
        with_session_owner_guards(
            BrowserPermissionGate::new(native, decider, authority).with_tab_revision_guard(
                move || {
                    app.state::<BrowserState>()
                        .settled_tab(&label)
                        .is_ok_and(|(_, current)| current == revision)
                },
            ),
            self.app.clone(),
            owner,
        )
        .perform(request)
    }
}

impl BrowserCapability for SessionBrowserCapability {
    fn attached_tab_id(&self) -> Option<u64> {
        let service = &self.app.state::<AppState>().acp;
        service.current_browser_owner(&self.project_id, &self.chat_id, &self.session_id)?;
        service
            .latest_agent_browser_attachment(
                &self.project_id,
                &self.chat_id,
                self.session_id.as_str(),
            )
            .map(|lease| lease.tab_number)
            .or_else(|| {
                service
                    .browser_attachment(&self.project_id, &self.chat_id)
                    .map(|lease| lease.tab_number)
            })
    }

    /// Take over pauses the call and Hand back resumes it (`pause.rs`).
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        let first = self.perform_once(request);
        if !super::pause::paused(&first, || self.user_holds_a_tab()) {
            return first;
        }
        let service = &self.app.state::<AppState>().acp;
        let Some(owner) =
            service.current_browser_owner(&self.project_id, &self.chat_id, &self.session_id)
        else {
            return first;
        };
        let resumed = super::pause::wait_for_hand_back(
            || self.user_holds_a_tab(),
            || {
                service
                    .current_browser_owner(&self.project_id, &self.chat_id, &self.session_id)
                    .is_some_and(|current| {
                        current.turn_id == owner.turn_id
                            && current.runtime_epoch == owner.runtime_epoch
                            && current.binding_digest == owner.binding_digest
                    })
            },
            std::time::Instant::now() + super::pause::HAND_BACK_WAIT,
            super::pause::POLL,
        );
        if resumed {
            self.perform_once(request)
        } else {
            Err(BrowserFailure::UserControl)
        }
    }
}
